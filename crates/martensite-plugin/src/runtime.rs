//! Wasmtime-powered sandboxed runtime for Martensite plugins.
//!
//! Plugins are compiled as `wasm32-wasip1` WebAssembly modules and executed
//! with a fuel budget and epoch interruption enabled. The runtime embeds a
//! WASIp1 context with no filesystem or network capabilities by default, and
//! injects host functions that validate every call against a
//! [`CapabilitySet`](crate::security::CapabilitySet).
//!
//! # Shared ring buffer ABI
//!
//! The paint-command ring buffer lives in guest-visible linear memory. A
//! plugin obtains it in one of two ways:
//!
//! - **Import** `(import "martensite" "ring_memory" (memory N))` with `N`
//!   pages covering [`RING_BUFFER_REGION_SIZE`]; the host supplies a dedicated
//!   memory whose region starts at offset `0`.
//! - **Export** `memory`; the host appends
//!   `ceil(RING_BUFFER_REGION_SIZE / 65536)` pages at instantiation and
//!   `martensite.ring_buffer_ptr` returns the region's base offset. Guests on
//!   this path must treat that range as reserved so their allocator never
//!   reuses it.
//!
//! The region layout is an 8-byte `head`/`tail` cursor header followed by
//! [`DEFAULT_CAPACITY`] bytes of circular payload area; see
//! [`PluginRingBuffer::new_shared`](crate::ring_buffer::PluginRingBuffer::new_shared).
//! `martensite.ring_buffer_len`, `martensite.ring_buffer_read`, and
//! `martensite.ring_buffer_write` host functions provide validated access for
//! guests that prefer not to touch the region directly.

use std::fmt;
use std::io::Read;

use wasmtime::{
    Caller, Config, Engine, Extern, ExternType, Instance, Linker, Memory, MemoryType, Module,
    Store, Trap, WasmParams, WasmResults,
};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use crate::ring_buffer::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY, SHARED_HEADER_SIZE};
use crate::security::{Capability, CapabilitySet};
use martensite_reactive::SignalId;

/// Default fuel budget allocated to each plugin instance.
///
/// This is a target budget for a roughly 5 ms execution slice, not a portable
/// time measurement. Wasmtime fuel costs are instruction-relative, so hosts
/// must calibrate this value for each target architecture and workload. Rogue
/// or stuck plugins are terminated when fuel is exhausted.
pub const DEFAULT_FUEL_BUDGET: u64 = 200_000;

/// Namespace used for Martensite-specific host functions exposed to plugins.
const HOST_NS: &str = "martensite";

/// Import name under which a plugin may request a host-provided ring buffer
/// memory (`(import "martensite" "ring_memory" (memory N))`).
const RING_MEMORY_IMPORT: &str = "ring_memory";

/// Number of bytes in one WebAssembly linear memory page.
const WASM_PAGE_SIZE: usize = 65_536;

/// Total byte size of the shared ring buffer region.
///
/// The region consists of a [`SHARED_HEADER_SIZE`]-byte cursor header
/// (`head: u32`, `tail: u32`, little-endian) followed by
/// [`DEFAULT_CAPACITY`] bytes of circular payload area. Guests that import
/// `martensite.ring_memory` must declare at least
/// `ceil(RING_BUFFER_REGION_SIZE / 65536)` pages.
///
/// # Examples
///
/// ```
/// use martensite_plugin::runtime::RING_BUFFER_REGION_SIZE;
/// use martensite_plugin::DEFAULT_CAPACITY;
///
/// assert_eq!(RING_BUFFER_REGION_SIZE, DEFAULT_CAPACITY + 8);
/// ```
pub const RING_BUFFER_REGION_SIZE: usize = SHARED_HEADER_SIZE + DEFAULT_CAPACITY;

/// Where the per-instance shared ring buffer region lives.
#[derive(Clone, Copy, Debug)]
enum RingLocation {
    /// A region appended to the guest's exported `memory` at instantiation.
    /// The payload is the byte offset of the region base.
    GuestMemory { base: u32 },
    /// A dedicated host-created memory imported as `martensite.ring_memory`.
    /// The region starts at offset zero of that memory.
    Imported(Memory),
}

/// Per-instance host state shared with the Wasmtime store.
///
/// Contains the WASIp1 context, the capability grants, and the shared ring
/// buffer location for the current plugin instance.
pub struct PluginState {
    wasi: WasiP1Ctx,
    caps: CapabilitySet,
    ring: Option<RingLocation>,
}

impl fmt::Debug for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginState")
            .field("capabilities", &self.caps)
            .finish_non_exhaustive()
    }
}

impl PluginState {
    /// Returns a reference to the capability set for this instance.
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.caps
    }
}

/// Errors that can occur while configuring or running a plugin.
#[derive(Debug)]
pub enum PluginError {
    /// An underlying Wasmtime error.
    Wasmtime(wasmtime::Error),
    /// The requested export was not found or had the wrong type.
    MissingExport(String),
    /// The plugin exhausted its fuel budget.
    OutOfFuel,
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::Wasmtime(e) => write!(f, "wasmtime error: {e}"),
            PluginError::MissingExport(name) => write!(f, "missing export: {name}"),
            PluginError::OutOfFuel => write!(f, "plugin ran out of fuel"),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<wasmtime::Error> for PluginError {
    fn from(err: wasmtime::Error) -> Self {
        if err
            .downcast_ref::<Trap>()
            .is_some_and(|t| matches!(t, Trap::OutOfFuel))
        {
            PluginError::OutOfFuel
        } else {
            PluginError::Wasmtime(err)
        }
    }
}

/// Maps an [`std::io::Error`] to a distinct negative `file_read` return code.
///
/// - `-1` for file-not-found (and as the generic fallback),
/// - `-3` for permission denied,
/// - `-4` for other I/O errors.
fn io_error_code(e: &std::io::Error) -> i32 {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => -1,
        ErrorKind::PermissionDenied => -3,
        _ => -4,
    }
}

/// Preconfigured Wasmtime runtime environment for loading plugins.
///
/// The engine is shared across instances; the linker is configured once with
/// the WASIp1 imports and Martensite host functions.
///
/// # Examples
///
/// ```
/// use martensite_plugin::{CapabilitySet, PluginRuntime};
///
/// let runtime = PluginRuntime::new().unwrap();
/// // The empty capability set gives the plugin no host access.
/// let _ = runtime.load(b"\0asm\x01\0\0\0", CapabilitySet::empty());
/// // (Loading a real wasm module would succeed; a minimal header is shown here.)
/// ```
pub struct PluginRuntime {
    engine: Engine,
    linker: Linker<PluginState>,
    fuel_budget: u64,
}

impl PluginRuntime {
    /// Creates a new runtime with the default fuel budget.
    ///
    /// # Errors
    ///
    /// Returns an error if the Wasmtime engine cannot be initialized.
    pub fn new() -> Result<Self, PluginError> {
        Self::with_fuel_budget(DEFAULT_FUEL_BUDGET)
    }

    /// Creates a new runtime with a custom fuel budget.
    ///
    /// # Errors
    ///
    /// Returns an error if the Wasmtime engine cannot be initialized.
    pub fn with_fuel_budget(fuel_budget: u64) -> Result<Self, PluginError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config)?;

        let mut linker = Linker::<PluginState>::new(&engine);
        preview1::add_to_linker_sync(&mut linker, |state: &mut PluginState| &mut state.wasi)?;
        Self::add_host_functions(&mut linker)?;

        Ok(Self {
            engine,
            linker,
            fuel_budget,
        })
    }

    fn add_host_functions(linker: &mut Linker<PluginState>) -> Result<(), PluginError> {
        linker.func_wrap(
            HOST_NS,
            "signal_read",
            |caller: Caller<'_, PluginState>, id: i64| {
                let state = caller.data();
                let cap = Capability::SignalRead(SignalId(id as u64));
                if state.caps.contains(&cap) {
                    Ok(())
                } else {
                    Err(wasmtime::Error::msg("unauthorized signal read"))
                }
            },
        )?;

        linker.func_wrap(
            HOST_NS,
            "signal_write",
            |caller: Caller<'_, PluginState>, id: i64| {
                let state = caller.data();
                let cap = Capability::SignalWrite(SignalId(id as u64));
                if state.caps.contains(&cap) {
                    Ok(())
                } else {
                    Err(wasmtime::Error::msg("unauthorized signal write"))
                }
            },
        )?;

        // `file_read(path_ptr, path_len, buf_ptr, buf_len) -> i32`
        //
        // Reads the file at the guest-supplied UTF-8 path into guest memory at
        // `buf_ptr`. Returns the number of bytes written on success. Negative
        // codes report specific failures:
        //   `-1` file not found (or generic fallback),
        //   `-2` file is larger than `buf_len`,
        //   `-3` permission denied,
        //   `-4` I/O error (including non-regular files),
        //   `-5` guest-supplied path or buffer length exceeds host limits.
        // The capability check runs before any filesystem access; unauthorized
        // paths trap the guest.
        linker.func_wrap(
            HOST_NS,
            "file_read",
            |mut caller: Caller<'_, PluginState>,
             path_ptr: i32,
             path_len: i32,
             buf_ptr: i32,
             buf_len: i32|
             -> Result<i32, wasmtime::Error> {
                let path_ptr = usize::try_from(path_ptr)
                    .map_err(|_| wasmtime::Error::msg("invalid file read path"))?;
                let path_len = usize::try_from(path_len)
                    .map_err(|_| wasmtime::Error::msg("invalid file read path"))?;
                let buf_ptr = usize::try_from(buf_ptr)
                    .map_err(|_| wasmtime::Error::msg("invalid file read buffer"))?;
                let buf_len = usize::try_from(buf_len)
                    .map_err(|_| wasmtime::Error::msg("invalid file read buffer"))?;

                // Bound guest-supplied lengths before allocating. A malicious
                // guest could otherwise request `i32::MAX` bytes and force a
                // ~2 GiB host allocation.
                const MAX_PATH_LEN: usize = 4096;
                const MAX_BUF_LEN: usize = 16 * 1024 * 1024;
                if path_len > MAX_PATH_LEN {
                    tracing::warn!(
                        len = path_len,
                        max = MAX_PATH_LEN,
                        "file_read: guest path length exceeds host limit"
                    );
                    return Ok(-5);
                }
                if buf_len > MAX_BUF_LEN {
                    tracing::warn!(
                        len = buf_len,
                        max = MAX_BUF_LEN,
                        "file_read: guest buffer length exceeds host limit"
                    );
                    return Ok(-5);
                }

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(memory)) => memory,
                    _ => return Err(wasmtime::Error::msg("missing guest memory")),
                };
                let mut path_bytes = vec![0; path_len];
                memory.read(&caller, path_ptr, &mut path_bytes)?;
                let path = std::str::from_utf8(&path_bytes)
                    .map_err(|_| wasmtime::Error::msg("invalid file read path"))?;

                // Fail-closed: authorization is checked before touching the
                // filesystem. The granted path (file or directory) must
                // canonically contain the requested path, defeating
                // traversal attacks like `/assets/../etc/passwd`.
                if !caller
                    .data()
                    .caps
                    .file_read_allowed(std::path::Path::new(path))
                {
                    return Err(wasmtime::Error::msg("unauthorized file read"));
                }

                // Open the file and inspect its metadata before reading. This
                // rejects non-regular files (FIFOs, device nodes, sockets)
                // which could otherwise hang the host on `read_to_end`, and
                // avoids reading the entire file when it is larger than the
                // guest-supplied buffer.
                let file = match std::fs::File::open(path) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!(error = %e, path, "file_read: open failed");
                        return Ok(io_error_code(&e));
                    }
                };
                let metadata = match file.metadata() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(error = %e, path, "file_read: metadata failed");
                        return Ok(io_error_code(&e));
                    }
                };
                if !metadata.file_type().is_file() {
                    tracing::warn!(path, "file_read: non-regular file rejected");
                    return Ok(-4);
                }
                if metadata.len() > buf_len as u64 {
                    // File is larger than the guest buffer; do not read.
                    return Ok(-2);
                }

                // `buf_len` is already bounded by `MAX_BUF_LEN`, so this
                // allocation is capped at 16 MiB.
                let mut contents = Vec::new();
                let mut limited = file.take(buf_len as u64);
                if let Err(e) = limited.read_to_end(&mut contents) {
                    tracing::warn!(error = %e, path, "file_read: read failed");
                    return Ok(io_error_code(&e));
                }

                match memory.write(&mut caller, buf_ptr, &contents) {
                    Ok(()) => i32::try_from(contents.len())
                        .map_err(|_| wasmtime::Error::msg("file too large")),
                    Err(e) => {
                        // Never silently drop a memory.write failure.
                        tracing::warn!(error = %e, "file_read: guest memory write failed");
                        Ok(-1)
                    }
                }
            },
        )?;

        linker.func_wrap(
            HOST_NS,
            "network_open",
            |caller: Caller<'_, PluginState>| {
                if caller.data().caps.contains(&Capability::Network) {
                    Ok(())
                } else {
                    Err(wasmtime::Error::msg("unauthorized network open"))
                }
            },
        )?;

        // `ring_buffer_ptr() -> i64`
        //
        // Returns the byte offset of the shared ring region. For guests with an
        // exported `memory` this is an offset into that memory; for guests that
        // import `martensite.ring_memory` it is `0` in the imported memory.
        // Traps when the plugin has no accessible ring region.
        linker.func_wrap(
            HOST_NS,
            "ring_buffer_ptr",
            |caller: Caller<'_, PluginState>| -> Result<i64, wasmtime::Error> {
                match caller.data().ring {
                    Some(RingLocation::GuestMemory { base }) => Ok(i64::from(base)),
                    Some(RingLocation::Imported(_)) => Ok(0),
                    None => Err(wasmtime::Error::msg(
                        "no ring buffer: export `memory` or import `martensite.ring_memory`",
                    )),
                }
            },
        )?;

        // `ring_buffer_capacity() -> i32`: payload capacity of the ring region.
        linker.func_wrap(
            HOST_NS,
            "ring_buffer_capacity",
            |caller: Caller<'_, PluginState>| -> Result<i32, wasmtime::Error> {
                match caller.data().ring {
                    Some(_) => i32::try_from(DEFAULT_CAPACITY)
                        .map_err(|_| wasmtime::Error::msg("capacity overflow")),
                    None => Err(wasmtime::Error::msg(
                        "no ring buffer: export `memory` or import `martensite.ring_memory`",
                    )),
                }
            },
        )?;

        // `ring_buffer_len() -> i64`: bytes currently stored in the ring.
        linker.func_wrap(
            HOST_NS,
            "ring_buffer_len",
            |mut caller: Caller<'_, PluginState>| -> Result<i64, wasmtime::Error> {
                let (memory, base) = Self::ring_memory(&mut caller)?;
                let mut header = [0u8; SHARED_HEADER_SIZE];
                memory.read(&caller, base, &mut header)?;
                let head = u32::from_le_bytes(header[0..4].try_into().unwrap_or_default());
                let tail = u32::from_le_bytes(header[4..8].try_into().unwrap_or_default());
                let cap = DEFAULT_CAPACITY as u32;
                if head > cap || tail > cap {
                    return Ok(0);
                }
                let len = if head == tail {
                    0
                } else if tail > head {
                    tail - head
                } else {
                    cap - head + tail
                };
                Ok(i64::from(len))
            },
        )?;

        // `ring_buffer_read(ring_offset, dst_ptr, len) -> i32`
        //
        // Copies `len` bytes from the ring region (offset relative to the
        // region base, including the cursor header) into the guest's linear
        // memory at `dst_ptr`. Returns `len`, or `-1` if the range is invalid.
        linker.func_wrap(
            HOST_NS,
            "ring_buffer_read",
            |mut caller: Caller<'_, PluginState>,
             ring_offset: i32,
             dst_ptr: i32,
             len: i32|
             -> Result<i32, wasmtime::Error> {
                let (memory, base) = Self::ring_memory(&mut caller)?;
                let (Ok(ring_offset), Ok(dst_ptr), Ok(len)) = (
                    usize::try_from(ring_offset),
                    usize::try_from(dst_ptr),
                    usize::try_from(len),
                ) else {
                    return Ok(-1);
                };
                if ring_offset.saturating_add(len) > RING_BUFFER_REGION_SIZE {
                    return Ok(-1);
                }
                let mut tmp = vec![0u8; len];
                if memory
                    .read(&caller, base.saturating_add(ring_offset), &mut tmp)
                    .is_err()
                {
                    return Ok(-1);
                }
                let guest = Self::guest_memory(&mut caller, memory);
                match guest.write(&mut caller, dst_ptr, &tmp) {
                    Ok(()) => {
                        i32::try_from(len).map_err(|_| wasmtime::Error::msg("ring read too large"))
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        // `ring_buffer_write(ring_offset, src_ptr, len) -> i32`
        //
        // Copies `len` bytes from the guest's linear memory at `src_ptr` into
        // the ring region at `ring_offset` relative to the region base.
        // Returns `len`, or `-1` if the range is invalid.
        linker.func_wrap(
            HOST_NS,
            "ring_buffer_write",
            |mut caller: Caller<'_, PluginState>,
             ring_offset: i32,
             src_ptr: i32,
             len: i32|
             -> Result<i32, wasmtime::Error> {
                let (memory, base) = Self::ring_memory(&mut caller)?;
                let (Ok(ring_offset), Ok(src_ptr), Ok(len)) = (
                    usize::try_from(ring_offset),
                    usize::try_from(src_ptr),
                    usize::try_from(len),
                ) else {
                    return Ok(-1);
                };
                if ring_offset.saturating_add(len) > RING_BUFFER_REGION_SIZE {
                    return Ok(-1);
                }
                let guest = Self::guest_memory(&mut caller, memory);
                let mut tmp = vec![0u8; len];
                if guest.read(&caller, src_ptr, &mut tmp).is_err() {
                    return Ok(-1);
                }
                match memory.write(&mut caller, base.saturating_add(ring_offset), &tmp) {
                    Ok(()) => {
                        i32::try_from(len).map_err(|_| wasmtime::Error::msg("ring write too large"))
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        Ok(())
    }

    /// Resolves the memory backing the shared ring region and the region's
    /// base offset within that memory.
    fn ring_memory(
        caller: &mut Caller<'_, PluginState>,
    ) -> Result<(Memory, usize), wasmtime::Error> {
        match caller.data().ring {
            Some(RingLocation::GuestMemory { base }) => match caller.get_export("memory") {
                Some(Extern::Memory(memory)) => Ok((memory, base as usize)),
                _ => Err(wasmtime::Error::msg("missing guest memory")),
            },
            Some(RingLocation::Imported(memory)) => Ok((memory, 0)),
            None => Err(wasmtime::Error::msg(
                "no ring buffer: export `memory` or import `martensite.ring_memory`",
            )),
        }
    }

    /// Returns the guest's primary linear memory: its exported `memory` when
    /// present, otherwise the ring memory itself (which is the guest's only
    /// address space when it imports `martensite.ring_memory`).
    fn guest_memory(caller: &mut Caller<'_, PluginState>, fallback: Memory) -> Memory {
        match caller.get_export("memory") {
            Some(Extern::Memory(memory)) => memory,
            _ => fallback,
        }
    }

    /// Returns the Wasmtime engine used by this runtime.
    ///
    /// Epoch interruption is configured for every plugin store. The host must
    /// call [`Engine::increment_epoch`] on this engine on a 5 ms cadence to
    /// enforce the wall-clock deadline in addition to the fuel budget.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Loads and instantiates a plugin with the given capability set.
    ///
    /// The returned [`PluginInstance`] is independent from the runtime and can
    /// be invoked repeatedly until its fuel budget is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if compilation, instantiation, or fuel setup fails.
    pub fn load(
        &self,
        wasm_bytes: &[u8],
        caps: CapabilitySet,
    ) -> Result<PluginInstance, PluginError> {
        let module = Module::new(&self.engine, wasm_bytes)?;

        // A plugin obtains its shared ring buffer one of two ways:
        //  1. It imports `martensite.ring_memory`, and the host supplies a
        //     dedicated memory sized to `RING_BUFFER_REGION_SIZE`. This is the
        //     preferred ABI because the region is guaranteed exclusive.
        //  2. It exports `memory`; the host grows that memory at
        //     instantiation time and uses the appended pages as the region.
        //     Guests on this path must treat
        //     `[ring_buffer_ptr(), ring_buffer_ptr() + RING_BUFFER_REGION_SIZE)`
        //     as reserved so their allocator never hands it out.
        let wants_ring_import = module.imports().any(|import| {
            import.module() == HOST_NS
                && import.name() == RING_MEMORY_IMPORT
                && matches!(import.ty(), ExternType::Memory(_))
        });

        let wasi = WasiCtxBuilder::new().build_p1();
        let state = PluginState {
            wasi,
            caps,
            ring: None,
        };
        let mut store = Store::new(&self.engine, state);
        store.set_fuel(self.fuel_budget)?;
        store.set_epoch_deadline(1);

        let ring_pages =
            u32::try_from(RING_BUFFER_REGION_SIZE.div_ceil(WASM_PAGE_SIZE)).unwrap_or(u32::MAX);

        let mut ring = None;
        let instance = if wants_ring_import {
            let memory = Memory::new(&mut store, MemoryType::new(ring_pages, None))?;
            // The shared linker cannot hold a per-store item, so clone it and
            // define the memory on the per-instance copy.
            let mut linker = self.linker.clone();
            linker.define(&mut store, HOST_NS, RING_MEMORY_IMPORT, memory)?;
            let instance = linker.instantiate(&mut store, &module)?;
            ring = Some(RingLocation::Imported(memory));
            instance
        } else {
            let instance = self.linker.instantiate(&mut store, &module)?;
            if let Some(memory) = instance.get_memory(&mut store, "memory") {
                let old_pages = memory.grow(&mut store, u64::from(ring_pages))?;
                let base = u32::try_from(old_pages as usize * WASM_PAGE_SIZE)
                    .map_err(|_| wasmtime::Error::msg("ring buffer offset overflow"))?;
                ring = Some(RingLocation::GuestMemory { base });
            }
            instance
        };
        store.data_mut().ring = ring;

        Ok(PluginInstance { store, instance })
    }
}

/// A running WebAssembly plugin instance.
///
/// Holds the Wasmtime [`Store`] and [`Instance`] for a single plugin. All
/// calls happen in the context of this instance and consume its fuel budget.
pub struct PluginInstance {
    store: Store<PluginState>,
    instance: Instance,
}

impl PluginInstance {
    /// Invokes an exported function that takes no parameters and returns
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::MissingExport`] if the export does not exist or
    /// has the wrong signature, or any other plugin error if execution fails.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_plugin::{CapabilitySet, PluginRuntime};
    ///
    /// let runtime = PluginRuntime::new().unwrap();
    /// let mut plugin = runtime.load(wasm_bytes, CapabilitySet::empty()).unwrap();
    /// plugin.invoke("run").unwrap();
    /// ```
    pub fn invoke(&mut self, name: &str) -> Result<(), PluginError> {
        self.invoke_typed(name, ())
    }

    /// Invokes an exported function with a statically checked WebAssembly
    /// signature.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::MissingExport`] if the export does not exist or
    /// has a signature different from `Args -> Rets`, or any execution error
    /// produced by the plugin.
    pub fn invoke_typed<Args, Rets>(&mut self, name: &str, args: Args) -> Result<Rets, PluginError>
    where
        Args: WasmParams,
        Rets: WasmResults,
    {
        let func = self
            .instance
            .get_typed_func::<Args, Rets>(&mut self.store, name)
            .map_err(|_| PluginError::MissingExport(name.to_string()))?;
        Ok(func.call(&mut self.store, args)?)
    }

    /// Drains paint commands from this instance's shared ring buffer.
    ///
    /// The ring region lives inside guest-visible linear memory: either a
    /// host-grown region of the guest's exported `memory`, or the dedicated
    /// `martensite.ring_memory` import. The guest produces commands by writing
    /// `PluginPaintCmd` records plus the `head`/`tail` cursor header directly;
    /// this call consumes and clears them. Does nothing when the plugin has no
    /// ring region.
    pub fn drain_paint_commands(&mut self, f: impl FnMut(&PluginPaintCmd, &[u8])) {
        let (memory, base) = match self.store.data().ring {
            Some(RingLocation::GuestMemory { base }) => {
                match self.instance.get_memory(&mut self.store, "memory") {
                    Some(memory) => (memory, base as usize),
                    None => return,
                }
            }
            Some(RingLocation::Imported(memory)) => (memory, 0),
            None => return,
        };
        let data = memory.data_mut(&mut self.store);
        let end = base.saturating_add(RING_BUFFER_REGION_SIZE).min(data.len());
        if end <= base {
            return;
        }
        let mut ring_buffer = PluginRingBuffer::new_shared(&mut data[base..end]);
        ring_buffer.drain(f);
    }

    /// Returns a reference to the capability set active for this instance.
    pub fn capabilities(&self) -> &CapabilitySet {
        self.store.data().capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn compile_wat(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).expect("valid WAT")
    }

    #[test]
    fn runtime_cannot_load_invalid_wasm() {
        let runtime = PluginRuntime::new().unwrap();
        assert!(runtime.load(b"not wasm", CapabilitySet::empty()).is_err());
    }

    #[test]
    fn guest_is_terminated_on_infinite_loop() {
        let wat = r#"
            (module
              (func (export "run")
                (loop (br 0))
              )
            )
        "#;
        let runtime = PluginRuntime::with_fuel_budget(10_000).unwrap();
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        let err = plugin.invoke("run").unwrap_err();
        assert!(
            matches!(err, PluginError::OutOfFuel),
            "expected OutOfFuel, got {err}"
        );
    }

    #[test]
    fn authorized_host_call_succeeds() {
        let wat = r#"
            (module
              (import "martensite" "signal_read" (func $signal_read (param i64)))
              (func (export "run")
                i64.const 42
                call $signal_read
              )
            )
        "#;
        let id = SignalId(42);
        let caps = CapabilitySet::builder()
            .grant(Capability::SignalRead(id))
            .build();

        let runtime = PluginRuntime::with_fuel_budget(50_000).unwrap();
        let mut plugin = runtime.load(&compile_wat(wat), caps).unwrap();
        plugin.invoke("run").unwrap();
    }

    #[test]
    fn unauthorized_host_call_traps() {
        let wat = r#"
            (module
              (import "martensite" "signal_read" (func $signal_read (param i64)))
              (func (export "run")
                i64.const 7
                call $signal_read
              )
            )
        "#;
        let runtime = PluginRuntime::with_fuel_budget(50_000).unwrap();
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        // The host function rejects the call because the capability was not
        // granted, so WebAssembly execution traps and returns an error.
        assert!(plugin.invoke("run").is_err());
    }

    /// Encodes a string as a WAT data-segment literal using `\xx` escapes so
    /// arbitrary path bytes can be embedded in a module.
    fn wat_str(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("\\{b:02x}")).collect()
    }

    #[test]
    fn file_read_requires_matching_path_capability() {
        let dir = std::env::temp_dir().join(format!("martensite-plugin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file_read_test.txt");
        std::fs::write(&path, b"hello plugin").unwrap();
        let path_str = path.to_string_lossy();
        let escaped = wat_str(path_str.as_bytes());
        let path_len = path_str.len();
        let wat = format!(
            r#"
            (module
              (import "martensite" "file_read"
                (func $file_read (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 16) "{escaped}")
              (func (export "run") (result i32)
                i32.const 16
                i32.const {path_len}
                i32.const 1024
                i32.const 64
                call $file_read
              )
              (func (export "buf") (param i32) (result i32)
                local.get 0
                i32.load8_u offset=1024)
            )
            "#
        );
        let wasm = compile_wat(&wat);
        let runtime = PluginRuntime::new().unwrap();
        let mut unauthorized = runtime.load(&wasm, CapabilitySet::empty()).unwrap();
        assert!(unauthorized.invoke_typed::<(), i32>("run", ()).is_err());

        // A different path must not satisfy the grant.
        let wrong_caps = CapabilitySet::builder()
            .grant(Capability::FileRead(dir.join("other.txt")))
            .build();
        let mut denied = runtime.load(&wasm, wrong_caps).unwrap();
        assert!(denied.invoke_typed::<(), i32>("run", ()).is_err());

        let caps = CapabilitySet::builder()
            .grant(Capability::FileRead(PathBuf::from(path_str.as_ref())))
            .build();
        let mut authorized = runtime.load(&wasm, caps).unwrap();
        let read = authorized.invoke_typed::<(), i32>("run", ()).unwrap();
        assert_eq!(read, 12);
        for (i, expected) in b"hello plugin".iter().enumerate() {
            let got = authorized
                .invoke_typed::<i32, i32>("buf", i as i32)
                .unwrap();
            assert_eq!(got as u8, *expected);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_read_reports_io_errors_and_oversized_buffers() {
        let dir = std::env::temp_dir().join(format!("martensite-plugin-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.bin");
        std::fs::write(&path, vec![7u8; 100]).unwrap();
        let path_str = path.to_string_lossy();
        let escaped = wat_str(path_str.as_bytes());
        let missing = dir.join("does_not_exist.bin");
        let missing_str = missing.to_string_lossy();
        let missing_escaped = wat_str(missing_str.as_bytes());
        let wat = format!(
            r#"
            (module
              (import "martensite" "file_read"
                (func $file_read (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 16) "{escaped}")
              (data (i32.const 4096) "{missing_escaped}")
              (func (export "small_buf") (result i32)
                i32.const 16 i32.const {} i32.const 2048 i32.const 10
                call $file_read)
              (func (export "missing") (result i32)
                i32.const 4096 i32.const {} i32.const 2048 i32.const 200
                call $file_read)
            )
            "#,
            path_str.len(),
            missing_str.len()
        );
        let caps = CapabilitySet::builder()
            .grant(Capability::FileRead(PathBuf::from(path_str.as_ref())))
            .grant(Capability::FileRead(PathBuf::from(missing_str.as_ref())))
            .build();
        let runtime = PluginRuntime::new().unwrap();
        let mut plugin = runtime.load(&compile_wat(&wat), caps).unwrap();
        assert_eq!(plugin.invoke_typed::<(), i32>("small_buf", ()).unwrap(), -2);
        assert_eq!(plugin.invoke_typed::<(), i32>("missing", ()).unwrap(), -1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn network_open_requires_capability() {
        let wat = r#"
            (module
              (import "martensite" "network_open" (func $network_open))
              (func (export "run") call $network_open)
            )
        "#;
        let runtime = PluginRuntime::new().unwrap();
        let mut unauthorized = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        assert!(unauthorized.invoke("run").is_err());

        let caps = CapabilitySet::builder().grant(Capability::Network).build();
        let mut authorized = runtime.load(&compile_wat(wat), caps).unwrap();
        authorized.invoke("run").unwrap();
    }

    #[test]
    fn typed_invoke_and_ring_buffer_exports_work() {
        let wat = r#"
            (module
              (import "martensite" "ring_buffer_ptr" (func $ptr (result i64)))
              (import "martensite" "ring_buffer_capacity" (func $capacity (result i32)))
              (import "martensite" "ring_buffer_len" (func $len (result i64)))
              (import "martensite" "ring_buffer_read"
                (func $read (param i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "add_one") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add)
              (func (export "ptr") (result i64) call $ptr)
              (func (export "capacity") (result i32) call $capacity)
              (func (export "ring_len") (result i64) call $len)
              ;; Produce one 4-byte-payload command directly into the region.
              (func (export "make_cmd")
                (local $p i32)
                call $ptr
                i32.wrap_i64
                local.set $p
                ;; head (already 0) at $p; tail = 16 at $p+4
                local.get $p i32.const 16 i32.store offset=4
                ;; record at $p+8: cmd_type=1|flags=0, data_len=4, offset=12
                local.get $p i32.const 1 i32.store offset=8
                local.get $p i32.const 4 i32.store offset=12
                local.get $p i32.const 12 i32.store offset=16
                ;; payload 0x0A0B0C0D at $p+20
                local.get $p i32.const 0x0A0B0C0D i32.store offset=20)
              ;; Copy 4 payload bytes out of the region via the host helper.
              (func (export "copy_out") (result i32)
                i32.const 20 i32.const 4096 i32.const 4
                call $read)
              (func (export "copied") (result i32)
                i32.const 4096 i32.load)
            )
        "#;
        let runtime = PluginRuntime::new().unwrap();
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        assert_eq!(plugin.invoke_typed::<i32, i32>("add_one", 41).unwrap(), 42);
        // The region is appended to the guest's single initial page.
        assert_eq!(
            plugin.invoke_typed::<(), i64>("ptr", ()).unwrap(),
            WASM_PAGE_SIZE as i64
        );
        assert_eq!(
            plugin.invoke_typed::<(), i32>("capacity", ()).unwrap(),
            DEFAULT_CAPACITY as i32
        );
        assert_eq!(plugin.invoke_typed::<(), i64>("ring_len", ()).unwrap(), 0);

        plugin.invoke("make_cmd").unwrap();
        assert_eq!(
            plugin.invoke_typed::<(), i64>("ring_len", ()).unwrap(),
            (PluginPaintCmd::header_size() + 4) as i64
        );
        assert_eq!(plugin.invoke_typed::<(), i32>("copy_out", ()).unwrap(), 4);
        assert_eq!(
            plugin.invoke_typed::<(), i32>("copied", ()).unwrap() as u32,
            0x0A0B0C0D
        );

        let mut seen = 0;
        plugin.drain_paint_commands(|cmd, payload| {
            seen += 1;
            assert_eq!(cmd.cmd_type, 1);
            assert_eq!(cmd.data_len, 4);
            assert_eq!(payload, &[0x0D, 0x0C, 0x0B, 0x0A]);
        });
        assert_eq!(seen, 1);
        // The cursor header was written back to guest memory on drain.
        assert_eq!(plugin.invoke_typed::<(), i64>("ring_len", ()).unwrap(), 0);
        assert!(std::ptr::eq(runtime.engine(), &runtime.engine));
    }

    #[test]
    fn imported_ring_memory_is_host_supplied() {
        let wat = r#"
            (module
              (import "martensite" "ring_memory" (memory 5))
              (import "martensite" "ring_buffer_ptr" (func $ptr (result i64)))
              (import "martensite" "ring_buffer_capacity" (func $cap (result i32)))
              (import "martensite" "ring_buffer_write"
                (func $write (param i32 i32 i32) (result i32)))
              (func (export "ptr") (result i64) call $ptr)
              (func (export "cap") (result i32) call $cap)
              ;; Write a command through the host helper: src is this same
              ;; memory (it is the guest's only address space).
              (func (export "make_cmd")
                ;; stage the record bytes in scratch space beyond the region
                i32.const 300000 i32.const 1 i32.store
                i32.const 300004 i32.const 4 i32.store
                i32.const 300008 i32.const 12 i32.store
                i32.const 300012 i32.const 0x11223344 i32.store
                ;; tail = 16
                i32.const 0 i32.const 16 i32.store offset=4
                ;; copy record header+payload into region offset 8
                i32.const 8 i32.const 300000 i32.const 16
                call $write
                drop)
            )
        "#;
        let runtime = PluginRuntime::new().unwrap();
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        assert_eq!(plugin.invoke_typed::<(), i64>("ptr", ()).unwrap(), 0);
        assert_eq!(
            plugin.invoke_typed::<(), i32>("cap", ()).unwrap(),
            DEFAULT_CAPACITY as i32
        );
        plugin.invoke("make_cmd").unwrap();
        let mut seen = 0;
        plugin.drain_paint_commands(|cmd, payload| {
            seen += 1;
            assert_eq!(cmd.cmd_type, 1);
            assert_eq!(payload, &[0x44, 0x33, 0x22, 0x11]);
        });
        assert_eq!(seen, 1);
    }

    #[test]
    fn ring_buffer_without_any_memory_traps() {
        let wat = r#"
            (module
              (import "martensite" "ring_buffer_ptr" (func $ptr (result i64)))
              (func (export "ptr") (result i64) call $ptr)
            )
        "#;
        let runtime = PluginRuntime::new().unwrap();
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        assert!(plugin.invoke_typed::<(), i64>("ptr", ()).is_err());
        // Draining is a no-op rather than a panic.
        plugin.drain_paint_commands(|_, _| panic!("no ring region exists"));
    }

    #[test]
    fn empty_capability_set_is_default_for_load() {
        let runtime = PluginRuntime::new().unwrap();
        let wat = r#"
            (module
              (func (export "run"))
            )
        "#;
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        plugin.invoke("run").unwrap();
        assert!(plugin.capabilities().is_empty());
    }
}
