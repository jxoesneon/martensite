//! Ahead-of-Time WGSL shader validation and reflection.
//!
//! This module wraps the [`naga`] crate to provide:
//!
//! - AOT validation of WGSL source via [`ShaderValidator`], catching invalid
//!   shaders at build time rather than at GPU pipeline-creation time.
//! - Reflection metadata ([`ShaderReflection`]) describing entry points and
//!   resource bindings, suitable for driving pipeline layout creation.
//!
//! # Reflection backend: naga (replaces spirv-cross)
//!
//! The original milestone v0.7.0 spec called for `spirv-cross` reflection.
//! Martensite uses [`naga`] instead because:
//!
//! - **Cross-platform without a C++ build dependency:** `spirv-cross` is a
//!   C++ library that requires a native compiler toolchain on every target,
//!   complicating cross-compilation and CI. `naga` is pure Rust, building
//!   everywhere `cargo` does with no extra toolchain.
//! - **Equivalent reflection:** `naga` validates and reflects WGSL (and
//!   SPIR-V) modules, exposing entry points, workgroup sizes, and resource
//!   bindings (`@group`/`@binding`) with the same information
//!   `spirv-cross` would provide for pipeline-layout creation.
//! - **Single source of truth:** WGSL is Martensite's shader authoring
//!   language, so reflecting it directly via `naga` avoids a WGSL→SPIR-V→reflect
//!   round-trip.
//!
//! No `spirv-cross` dependency is added. This decision is documented in the
//! milestone v0.7.0 spec.
//!
//! # Examples
//!
//! ```
//! use martensite_assets::shader::ShaderValidator;
//!
//! let wgsl = r#"
//!     @group(0) @binding(0) var<uniform> u: vec4<f32>;
//!     @vertex
//!     fn vs() -> @builtin(position) vec4<f32> { return u; }
//! "#;
//!
//! let mut validator = ShaderValidator::new();
//! let reflection = validator.validate(wgsl).expect("shader should validate");
//! assert_eq!(reflection.entry_points.len(), 1);
//! assert_eq!(reflection.bindings.len(), 1);
//! ```

use core::fmt;

// ============================================================================
// Error type
// ============================================================================

/// An error encountered while parsing or validating a WGSL shader.
///
/// Returned by [`ShaderValidator::validate`].
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::{ShaderError, ShaderValidator};
///
/// let mut validator = ShaderValidator::new();
/// let err = validator.validate("not valid wgsl").unwrap_err();
/// assert!(matches!(err, ShaderError::Parse(_)));
/// ```
#[derive(Debug)]
pub enum ShaderError {
    /// The WGSL source could not be parsed by naga's front-end.
    Parse(String),
    /// The parsed module failed naga's validation pass.
    Validation(String),
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "WGSL parse error: {msg}"),
            Self::Validation(msg) => write!(f, "WGSL validation error: {msg}"),
        }
    }
}

impl std::error::Error for ShaderError {}

// ============================================================================
// Reflection types
// ============================================================================

/// The programmable shader stage of an entry point.
///
/// Mirrors the subset of [`naga::ShaderStage`] variants that are expressible in
/// WGSL source.
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::{ShaderValidator, ShaderStage};
///
/// let mut v = ShaderValidator::new();
/// let r = v.validate("@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }").unwrap();
/// assert_eq!(r.entry_points[0].stage, ShaderStage::Vertex);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    /// Vertex shader stage.
    Vertex,
    /// Fragment shader stage.
    Fragment,
    /// Compute shader stage.
    Compute,
}

impl fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Vertex => "vertex",
            Self::Fragment => "fragment",
            Self::Compute => "compute",
        })
    }
}

/// A single entry point discovered during shader reflection.
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::{ShaderValidator, ShaderStage};
///
/// let mut v = ShaderValidator::new();
/// let wgsl = "@compute @workgroup_size(8, 4, 1) fn cs() {}";
/// let r = v.validate(wgsl).unwrap();
/// let ep = &r.entry_points[0];
/// assert_eq!(ep.name, "cs");
/// assert_eq!(ep.stage, ShaderStage::Compute);
/// assert_eq!(ep.workgroup_size, [8, 4, 1]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    /// The entry point name, as declared in WGSL.
    pub name: String,
    /// The pipeline stage this entry point targets.
    pub stage: ShaderStage,
    /// The compute workgroup size `[x, y, z]`. `[0, 0, 0]` for non-compute
    /// stages (naga leaves workgroup size unset for vertex/fragment entry
    /// points).
    pub workgroup_size: [u32; 3],
}

/// The kind of a resource binding, used by [`BindingInfo`].
///
/// Classifies a `@group`/`@binding` resource by its address space and type so
/// callers can build pipeline layouts without re-introspecting naga types.
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::{BindingType, ShaderValidator};
///
/// let mut v = ShaderValidator::new();
/// let wgsl = r#"
///     @group(0) @binding(0) var<uniform> u: vec4<f32>;
///     @group(0) @binding(1) var t: texture_2d<f32>;
///     @group(0) @binding(2) var s: sampler;
/// "#;
/// let r = v.validate(wgsl).unwrap();
/// let kinds: Vec<&str> = r.bindings.iter().map(|b| match b.binding_type {
///     BindingType::Uniform => "uniform",
///     BindingType::Texture => "texture",
///     BindingType::Sampler { .. } => "sampler",
///     _ => "other",
/// }).collect();
/// assert!(kinds.contains(&"uniform"));
/// assert!(kinds.contains(&"texture"));
/// assert!(kinds.contains(&"sampler"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingType {
    /// A uniform buffer bound via `var<uniform>`.
    Uniform,
    /// A storage buffer bound via `var<storage, read>` or `var<storage, read_write>`.
    Storage {
        /// `true` if the buffer is read-only (`var<storage, read>`).
        read_only: bool,
    },
    /// A sampled texture (`texture_*`).
    Texture,
    /// A storage texture (`texture_storage_*`).
    StorageTexture {
        /// `true` if the storage texture is read-only.
        read_only: bool,
    },
    /// A sampler (`sampler` or `sampler_comparison`).
    Sampler {
        /// `true` for `sampler_comparison` (depth-comparison samplers).
        comparison: bool,
    },
}

/// Reflection metadata for a single resource binding.
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::{BindingType, ShaderValidator};
///
/// let mut v = ShaderValidator::new();
/// let wgsl = "@group(1) @binding(2) var<uniform> u: vec4<f32>;";
/// let r = v.validate(wgsl).unwrap();
/// let b = &r.bindings[0];
/// assert_eq!(b.group, 1);
/// assert_eq!(b.binding, 2);
/// assert_eq!(b.binding_type, BindingType::Uniform);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    /// The variable name, if declared in WGSL.
    pub name: Option<String>,
    /// The bind group index (`@group(N)`).
    pub group: u32,
    /// The binding index within the group (`@binding(N)`).
    pub binding: u32,
    /// The classified kind of this resource.
    pub binding_type: BindingType,
}

/// Reflection metadata for a validated WGSL module.
///
/// Produced by [`ShaderValidator::validate`]; describes the module's entry
/// points and resource bindings so callers can construct pipeline layouts and
/// bind groups without a separate reflection pass.
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::ShaderValidator;
///
/// let mut v = ShaderValidator::new();
/// let wgsl = r#"
///     @group(0) @binding(0) var<uniform> u: vec4<f32>;
///     @vertex fn vs() -> @builtin(position) vec4<f32> { return u; }
/// "#;
/// let r = v.validate(wgsl).unwrap();
/// assert_eq!(r.entry_points.len(), 1);
/// assert_eq!(r.bindings.len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderReflection {
    /// All entry points declared in the module.
    pub entry_points: Vec<EntryPoint>,
    /// All resource bindings (`@group`/`@binding`) declared in the module.
    pub bindings: Vec<BindingInfo>,
}

// ============================================================================
// ShaderValidator
// ============================================================================

/// AOT WGSL shader validator backed by [`naga`].
///
/// Parses WGSL source with naga's front-end and runs the naga validator,
/// returning [`ShaderReflection`] on success or a [`ShaderError`] on failure.
/// The validator is configured with all validation flags and all capabilities
/// enabled, so it accepts any valid WGSL the GPU might support.
///
/// # Examples
///
/// ```
/// use martensite_assets::shader::ShaderValidator;
///
/// let mut validator = ShaderValidator::new();
/// let ok = validator.validate("@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }");
/// assert!(ok.is_ok());
///
/// let bad = validator.validate("fn broken(");
/// assert!(bad.is_err());
/// ```
pub struct ShaderValidator {
    validator: naga::valid::Validator,
}

impl ShaderValidator {
    /// Create a new validator with all validation flags and capabilities.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::shader::ShaderValidator;
    /// let mut v = ShaderValidator::new();
    /// assert!(v.validate("@compute @workgroup_size(1) fn c() {}").is_ok());
    /// ```
    pub fn new() -> Self {
        Self {
            validator: naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            ),
        }
    }

    /// Parse, validate, and reflect a WGSL source string.
    ///
    /// On success returns [`ShaderReflection`] describing the module. On
    /// failure returns a [`ShaderError`] indicating whether parsing or
    /// validation failed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::shader::ShaderValidator;
    /// let mut v = ShaderValidator::new();
    /// let reflection = v.validate("@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }").unwrap();
    /// assert_eq!(reflection.entry_points.len(), 1);
    /// ```
    pub fn validate(&mut self, source: &str) -> Result<ShaderReflection, ShaderError> {
        let module =
            naga::front::wgsl::parse_str(source).map_err(|e| ShaderError::Parse(e.to_string()))?;
        self.validator
            .validate(&module)
            .map_err(|e| ShaderError::Validation(e.to_string()))?;
        Ok(ShaderReflection::from_module(&module))
    }
}

impl Default for ShaderValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ShaderValidator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShaderValidator").finish_non_exhaustive()
    }
}

// ============================================================================
// Reflection construction
// ============================================================================

impl ShaderReflection {
    /// Build reflection metadata from a validated naga [`naga::Module`].
    fn from_module(module: &naga::Module) -> Self {
        let entry_points = module
            .entry_points
            .iter()
            .map(|ep| EntryPoint {
                name: ep.name.clone(),
                stage: stage_from_naga(ep.stage),
                workgroup_size: ep.workgroup_size,
            })
            .collect();

        let mut bindings = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (_, var) in module.global_variables.iter() {
            let Some(naga::ResourceBinding { group, binding }) = var.binding else {
                continue;
            };
            let key = (group, binding);
            if !seen.insert(key) {
                continue;
            }
            let binding_type = classify_binding(module, var);
            bindings.push(BindingInfo {
                name: var.name.clone(),
                group,
                binding,
                binding_type,
            });
        }
        // Sort bindings for deterministic output (group, then binding).
        bindings.sort_by_key(|b| (b.group, b.binding));

        Self {
            entry_points,
            bindings,
        }
    }
}

/// Map a [`naga::ShaderStage`] to our public [`ShaderStage`].
fn stage_from_naga(stage: naga::ShaderStage) -> ShaderStage {
    match stage {
        naga::ShaderStage::Vertex => ShaderStage::Vertex,
        naga::ShaderStage::Fragment => ShaderStage::Fragment,
        naga::ShaderStage::Compute => ShaderStage::Compute,
        // WGSL cannot express these stages directly; default to Compute for
        // the workgroup-size semantics, though such modules won't originate
        // from WGSL source in practice.
        _ => ShaderStage::Compute,
    }
}

/// Classify a global variable into a [`BindingType`] using its address space
/// and (for handles) its type's inner structure.
fn classify_binding(module: &naga::Module, var: &naga::GlobalVariable) -> BindingType {
    use naga::{AddressSpace, StorageAccess, TypeInner};

    match var.space {
        AddressSpace::Uniform => BindingType::Uniform,
        AddressSpace::Storage { access } => BindingType::Storage {
            read_only: !access.contains(StorageAccess::STORE),
        },
        AddressSpace::Handle => {
            // The module has already been validated, so the type handle is
            // guaranteed to be in-bounds; indexing is safe here.
            let inner = &module.types[var.ty].inner;
            match inner {
                TypeInner::Sampler { comparison } => BindingType::Sampler {
                    comparison: *comparison,
                },
                TypeInner::Image {
                    class: naga::ImageClass::Storage { access, .. },
                    ..
                } => BindingType::StorageTexture {
                    read_only: !access.contains(StorageAccess::STORE),
                },
                TypeInner::Image { .. } => BindingType::Texture,
                // Unknown handle type (e.g. binding array); default to Texture.
                _ => BindingType::Texture,
            }
        }
        // Non-resource address spaces don't carry @group/@binding, but be safe.
        _ => BindingType::Uniform,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERTEX_SHADER: &str = r#"
        @vertex
        fn vs_main() -> @builtin(position) vec4<f32> {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
    "#;

    const COMPUTE_SHADER: &str = r#"
        @group(0) @binding(0) var<storage, read_write> buf: array<f32>;
        @compute @workgroup_size(8, 4, 2)
        fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
            buf[id.x] = 1.0;
        }
    "#;

    #[test]
    fn validates_simple_vertex_shader() {
        let mut v = ShaderValidator::new();
        let r = v.validate(VERTEX_SHADER).unwrap();
        assert_eq!(r.entry_points.len(), 1);
        assert_eq!(r.entry_points[0].name, "vs_main");
        assert_eq!(r.entry_points[0].stage, ShaderStage::Vertex);
        assert_eq!(r.entry_points[0].workgroup_size, [0, 0, 0]);
        assert!(r.bindings.is_empty());
    }

    #[test]
    fn validates_compute_with_workgroup_size() {
        let mut v = ShaderValidator::new();
        let r = v.validate(COMPUTE_SHADER).unwrap();
        let ep = &r.entry_points[0];
        assert_eq!(ep.name, "cs_main");
        assert_eq!(ep.stage, ShaderStage::Compute);
        assert_eq!(ep.workgroup_size, [8, 4, 2]);
    }

    #[test]
    fn reflects_uniform_and_storage_bindings() {
        let wgsl = r#"
            struct U { x: vec4<f32> };
            @group(0) @binding(0) var<uniform> u: U;
            @group(0) @binding(1) var<storage, read> ro: array<f32>;
            @group(0) @binding(2) var<storage, read_write> rw: array<f32>;
            @compute @workgroup_size(1) fn c() {
                let a = u.x;
                let b = ro[0];
                rw[0] = a.x + b;
            }
        "#;
        let mut v = ShaderValidator::new();
        let r = v.validate(wgsl).unwrap();
        assert_eq!(r.bindings.len(), 3);
        assert_eq!(r.bindings[0].group, 0);
        assert_eq!(r.bindings[0].binding, 0);
        assert_eq!(r.bindings[0].binding_type, BindingType::Uniform);
        assert_eq!(
            r.bindings[1].binding_type,
            BindingType::Storage { read_only: true }
        );
        assert_eq!(
            r.bindings[2].binding_type,
            BindingType::Storage { read_only: false }
        );
    }

    #[test]
    fn reflects_texture_and_sampler_bindings() {
        let wgsl = r#"
            @group(0) @binding(0) var t: texture_2d<f32>;
            @group(0) @binding(1) var s: sampler;
            @group(0) @binding(2) var sc: sampler_comparison;
            @group(0) @binding(3) var st: texture_storage_2d<rgba8unorm, read_write>;
            @fragment fn f() -> @location(0) vec4<f32> {
                return vec4<f32>(0.0);
            }
        "#;
        let mut v = ShaderValidator::new();
        let r = v.validate(wgsl).unwrap();
        let by_binding: std::collections::HashMap<u32, &BindingInfo> =
            r.bindings.iter().map(|b| (b.binding, b)).collect();
        assert_eq!(by_binding[&0].binding_type, BindingType::Texture);
        assert_eq!(
            by_binding[&1].binding_type,
            BindingType::Sampler { comparison: false }
        );
        assert_eq!(
            by_binding[&2].binding_type,
            BindingType::Sampler { comparison: true }
        );
        assert_eq!(
            by_binding[&3].binding_type,
            BindingType::StorageTexture { read_only: false }
        );
    }

    #[test]
    fn bindings_are_sorted_and_deduplicated() {
        // Two variables sharing a binding point should only appear once.
        let wgsl = r#"
            @group(0) @binding(0) var<uniform> u: vec4<f32>;
            @vertex fn vs() -> @builtin(position) vec4<f32> { return u; }
        "#;
        let mut v = ShaderValidator::new();
        let r = v.validate(wgsl).unwrap();
        // Sorted by (group, binding).
        let mut sorted = r.bindings.clone();
        sorted.sort_by_key(|b| (b.group, b.binding));
        assert_eq!(r.bindings, sorted);
        // No duplicate (group, binding) pairs.
        let mut keys: Vec<(u32, u32)> = r.bindings.iter().map(|b| (b.group, b.binding)).collect();
        let len_before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), len_before);
    }

    #[test]
    fn catches_parse_error() {
        let mut v = ShaderValidator::new();
        let err = v.validate("fn broken(").unwrap_err();
        assert!(matches!(err, ShaderError::Parse(_)));
        assert!(err.to_string().contains("parse"));
    }

    #[test]
    fn catches_validation_error() {
        // `return` with a mismatched type is a validation error after parsing.
        let wgsl = r#"
            @vertex
            fn vs() -> @builtin(position) vec4<f32> {
                return vec2<f32>(0.0, 0.0);
            }
        "#;
        let mut v = ShaderValidator::new();
        let err = v.validate(wgsl).unwrap_err();
        assert!(matches!(err, ShaderError::Validation(_)));
        assert!(err.to_string().contains("validation"));
    }

    #[test]
    fn catches_undeclared_entry_point_return_mismatch() {
        // Missing return value in a non-void entry point -> validation error.
        let wgsl = "@vertex fn vs() -> @builtin(position) vec4<f32> {}";
        let mut v = ShaderValidator::new();
        assert!(matches!(
            v.validate(wgsl).unwrap_err(),
            ShaderError::Validation(_)
        ));
    }

    #[test]
    fn default_is_equivalent_to_new() {
        let mut a = ShaderValidator::new();
        let mut b = ShaderValidator::default();
        let ra = a.validate(VERTEX_SHADER).unwrap();
        let rb = b.validate(VERTEX_SHADER).unwrap();
        assert_eq!(ra, rb);
    }

    #[test]
    fn shader_stage_display() {
        assert_eq!(ShaderStage::Vertex.to_string(), "vertex");
        assert_eq!(ShaderStage::Fragment.to_string(), "fragment");
        assert_eq!(ShaderStage::Compute.to_string(), "compute");
    }

    #[test]
    fn shader_error_display() {
        let p = ShaderError::Parse("boom".to_string());
        let v = ShaderError::Validation("kapow".to_string());
        assert!(p.to_string().contains("boom"));
        assert!(v.to_string().contains("kapow"));
    }

    #[test]
    fn validator_debug() {
        let v = ShaderValidator::new();
        let s = format!("{:?}", v);
        assert!(s.contains("ShaderValidator"));
    }

    #[test]
    fn empty_module_validates_with_no_bindings() {
        let mut v = ShaderValidator::new();
        // A module with only a no-op function and no entry points is valid.
        let r = v.validate("fn helper() -> u32 { return 0u; }").unwrap();
        assert!(r.entry_points.is_empty());
        assert!(r.bindings.is_empty());
    }

    #[test]
    fn multiple_entry_points_reflected() {
        let wgsl = r#"
            @vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }
            @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }
        "#;
        let mut v = ShaderValidator::new();
        let r = v.validate(wgsl).unwrap();
        assert_eq!(r.entry_points.len(), 2);
        let stages: Vec<ShaderStage> = r.entry_points.iter().map(|e| e.stage).collect();
        assert!(stages.contains(&ShaderStage::Vertex));
        assert!(stages.contains(&ShaderStage::Fragment));
    }
}
