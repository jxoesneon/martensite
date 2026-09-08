//! Wasmtime-powered sandboxed runtime for Martensite plugins.
//!
//! Plugins are compiled as `wasm32-wasip1` WebAssembly modules and executed
//! with a fuel budget and epoch interruption enabled. The runtime embeds a
//! WASIp1 context with no filesystem or network capabilities by default, and
//! injects host functions that validate every call against a
//! [`CapabilitySet`](crate::security::CapabilitySet).

use std::fmt;
use std::path::PathBuf;

use wasmtime::{
    Caller, Config, Engine, Extern, Instance, Linker, Module, Store, Trap, WasmParams, WasmResults,
};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use crate::ring_buffer::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY};
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

/// Per-instance host state shared with the Wasmtime store.
///
/// Contains the WASIp1 context and the capability grants for the current
/// plugin instance.
pub struct PluginState {
    wasi: WasiP1Ctx,
    caps: CapabilitySet,
    ring_buffer: Vec<u8>,
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

        linker.func_wrap(
            HOST_NS,
            "file_read",
            |mut caller: Caller<'_, PluginState>,
             path_ptr: i32,
             path_len: i32,
             _buf_ptr: i32,
             _buf_len: i32| {
                let path_ptr = usize::try_from(path_ptr)
                    .map_err(|_| wasmtime::Error::msg("invalid file read path"))?;
                let path_len = usize::try_from(path_len)
                    .map_err(|_| wasmtime::Error::msg("invalid file read path"))?;
                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(memory)) => memory,
                    _ => return Err(wasmtime::Error::msg("missing guest memory")),
                };
                let mut path_bytes = vec![0; path_len];
                memory.read(&caller, path_ptr, &mut path_bytes)?;
                let path = std::str::from_utf8(&path_bytes)
                    .map_err(|_| wasmtime::Error::msg("invalid file read path"))?;
                if !caller
                    .data()
                    .caps
                    .contains(&Capability::FileRead(PathBuf::from(path)))
                {
                    return Err(wasmtime::Error::msg("unauthorized file read"));
                }

                // Actual filesystem I/O will be delegated to a future WasiCtx
                // preopen; this host function currently enforces authorization.
                Ok(())
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

        linker.func_wrap(HOST_NS, "ring_buffer_ptr", || -> i64 { 0 })?;
        linker.func_wrap(HOST_NS, "ring_buffer_capacity", || -> i32 {
            DEFAULT_CAPACITY as i32
        })?;

        Ok(())
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

        let wasi = WasiCtxBuilder::new().build_p1();
        let state = PluginState {
            wasi,
            caps,
            ring_buffer: vec![0; DEFAULT_CAPACITY],
        };
        let mut store = Store::new(&self.engine, state);
        store.set_fuel(self.fuel_budget)?;
        store.set_epoch_deadline(1);

        let instance = self.linker.instantiate(&mut store, &module)?;

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

    /// Drains paint commands from this instance's host-side ring buffer.
    ///
    /// The current ABI reports offset zero to the guest and keeps the backing
    /// allocation in host state. Mapping it directly into guest linear memory
    /// is reserved as a future shared-memory optimization.
    pub fn drain_paint_commands(&mut self, f: impl FnMut(&PluginPaintCmd, &[u8])) {
        let mut ring_buffer = PluginRingBuffer::new(&mut self.store.data_mut().ring_buffer);
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

    #[test]
    fn file_read_requires_matching_path_capability() {
        let wat = r#"
            (module
              (import "martensite" "file_read"
                (func $file_read (param i32 i32 i32 i32)))
              (memory (export "memory") 1)
              (data (i32.const 16) "/assets")
              (func (export "run")
                i32.const 16
                i32.const 7
                i32.const 0
                i32.const 0
                call $file_read
              )
            )
        "#;
        let runtime = PluginRuntime::new().unwrap();
        let mut unauthorized = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        assert!(unauthorized.invoke("run").is_err());

        let caps = CapabilitySet::builder()
            .grant(Capability::FileRead(PathBuf::from("/assets")))
            .build();
        let mut authorized = runtime.load(&compile_wat(wat), caps).unwrap();
        authorized.invoke("run").unwrap();
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
              (func (export "add_one") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add)
              (func (export "ptr") (result i64) call $ptr)
              (func (export "capacity") (result i32) call $capacity)
            )
        "#;
        let runtime = PluginRuntime::new().unwrap();
        let mut plugin = runtime
            .load(&compile_wat(wat), CapabilitySet::empty())
            .unwrap();
        assert_eq!(plugin.invoke_typed::<i32, i32>("add_one", 41).unwrap(), 42);
        assert_eq!(plugin.invoke_typed::<(), i64>("ptr", ()).unwrap(), 0);
        assert_eq!(
            plugin.invoke_typed::<(), i32>("capacity", ()).unwrap(),
            DEFAULT_CAPACITY as i32
        );
        plugin.drain_paint_commands(|_, _| panic!("new buffer must be empty"));
        assert!(std::ptr::eq(runtime.engine(), &runtime.engine));
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
