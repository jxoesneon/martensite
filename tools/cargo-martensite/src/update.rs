//! Signed update manifest handling and self-update execution.
//!
//! This module provides cryptographic verification of update manifests and
//! binary release assets using Ed25519 signatures and SHA-256 digests. It also
//! implements the `cargo martensite self-update` command with atomic executable
//! replacement and rollback support.
//!
//! # Examples
//!
//! ```
//! use cargo_martensite::update::{
//!     generate_keypair, sign_asset, sign_manifest, verify_asset, verify_manifest,
//!     ReleaseAsset, UpdateManifest,
//! };
//! use std::collections::HashMap;
//!
//! let (signing_key, verifying_key) = generate_keypair();
//! let payload = b"new binary contents";
//! let (sha256, asset_sig) = sign_asset(&signing_key, payload);
//!
//! let mut assets = HashMap::new();
//! assets.insert(
//!     "x86_64-pc-windows-msvc".to_string(),
//!     ReleaseAsset {
//!         url: "file://binary.exe".to_string(),
//!         sha256,
//!         size: payload.len() as u64,
//!         signature: asset_sig,
//!     },
//! );
//!
//! let mut manifest = UpdateManifest {
//!     version: "0.20.0".to_string(),
//!     release_date: "2026-10-01".to_string(),
//!     release_notes: "Initial release".to_string(),
//!     min_version: None,
//!     assets,
//!     signature: None,
//! };
//!
//! sign_manifest(&mut manifest, &signing_key).unwrap();
//! assert!(verify_manifest(&manifest, &verifying_key).is_ok());
//! assert!(verify_asset(&manifest, "x86_64-pc-windows-msvc", payload, &verifying_key).is_ok());
//! ```

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;

/// The target triple of the current compilation host.
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
pub const CURRENT_TARGET: &str = "x86_64-pc-windows-msvc";

/// The target triple of the current compilation host.
#[cfg(all(target_arch = "aarch64", target_os = "windows"))]
pub const CURRENT_TARGET: &str = "aarch64-pc-windows-msvc";

/// The target triple of the current compilation host.
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
pub const CURRENT_TARGET: &str = "x86_64-apple-darwin";

/// The target triple of the current compilation host.
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub const CURRENT_TARGET: &str = "aarch64-apple-darwin";

/// The target triple of the current compilation host.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub const CURRENT_TARGET: &str = "x86_64-unknown-linux-gnu";

/// The target triple of the current compilation host.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub const CURRENT_TARGET: &str = "aarch64-unknown-linux-gnu";

/// Fallback host triple when not matching the standard desktop architectures.
#[cfg(not(any(
    all(target_arch = "x86_64", target_os = "windows"),
    all(target_arch = "aarch64", target_os = "windows"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux"),
    all(target_arch = "aarch64", target_os = "linux"),
)))]
pub const CURRENT_TARGET: &str = "unknown-target";

/// Returns the target triple of the current host platform.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::current_target;
///
/// let target = current_target();
/// assert!(!target.is_empty());
/// ```
pub fn current_target() -> &'static str {
    CURRENT_TARGET
}

/// Official default Ed25519 public key (hex-encoded) used to verify distribution updates.
///
/// Derived from the official release signing seed for Martensite v0.19.0.
pub const DEFAULT_PUBLIC_KEY_HEX: &str =
    "ce23d0983b47915b61dc66d45b105262de3aef53870f38c453784b23fa4137ef";

/// Deterministic release seed used for testing and default key derivation.
pub const OFFICIAL_RELEASE_SEED: [u8; 32] = *b"martensite.distribution.v0.19.0_";

/// A downloadable release asset for a specific target platform.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::ReleaseAsset;
///
/// let asset = ReleaseAsset {
///     url: "https://example.com/cargo-martensite.exe".to_string(),
///     sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
///     size: 1024,
///     signature: "00".repeat(64),
/// };
/// assert_eq!(asset.size, 1024);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// Download URL or local file path for the binary artifact.
    pub url: String,
    /// Hex-encoded SHA-256 digest of the uncompressed or archive payload.
    pub sha256: String,
    /// Size of the asset payload in bytes.
    pub size: u64,
    /// Hex-encoded Ed25519 signature over the payload or manifest.
    pub signature: String,
}

/// An update manifest describing available releases, assets, and cryptographic signatures.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{ReleaseAsset, UpdateManifest};
/// use std::collections::HashMap;
///
/// let manifest = UpdateManifest {
///     version: "0.20.0".to_string(),
///     release_date: "2026-10-01".to_string(),
///     release_notes: "Fixes and performance improvements".to_string(),
///     min_version: Some("0.18.0".to_string()),
///     assets: HashMap::new(),
///     signature: None,
/// };
/// assert_eq!(manifest.version, "0.20.0");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateManifest {
    /// Target release version string (semver).
    pub version: String,
    /// Release date in ISO 8601 (YYYY-MM-DD).
    pub release_date: String,
    /// Release notes or changelog summary.
    pub release_notes: String,
    /// Minimum required previous version to update from (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    /// Available release assets keyed by target triple (e.g. `x86_64-pc-windows-msvc`).
    pub assets: HashMap<String, ReleaseAsset>,
    /// Hex-encoded Ed25519 signature over canonical manifest JSON payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Serialize)]
struct CanonicalManifest<'a> {
    version: &'a str,
    release_date: &'a str,
    release_notes: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<&'a str>,
    assets: BTreeMap<&'a str, &'a ReleaseAsset>,
}

impl UpdateManifest {
    /// Returns the deterministic canonical JSON bytes of this manifest (excluding the signature).
    ///
    /// The canonical format uses sorted asset keys and omits optional `null` fields to ensure
    /// reproducible cryptographic signing and verification across platforms.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::UpdateManifest;
    /// use std::collections::HashMap;
    ///
    /// let manifest = UpdateManifest {
    ///     version: "0.20.0".to_string(),
    ///     release_date: "2026-10-01".to_string(),
    ///     release_notes: "Initial".to_string(),
    ///     min_version: None,
    ///     assets: HashMap::new(),
    ///     signature: None,
    /// };
    /// let bytes = manifest.canonical_bytes().unwrap();
    /// assert!(!bytes.is_empty());
    /// ```
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, UpdateError> {
        let mut sorted_assets = BTreeMap::new();
        for (k, v) in &self.assets {
            sorted_assets.insert(k.as_str(), v);
        }
        let canonical = CanonicalManifest {
            version: &self.version,
            release_date: &self.release_date,
            release_notes: &self.release_notes,
            min_version: self.min_version.as_deref(),
            assets: sorted_assets,
        };
        serde_json::to_vec(&canonical).map_err(|e| UpdateError::ManifestFormat(e.to_string()))
    }

    /// Signs the manifest using the provided Ed25519 signing key.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::{generate_keypair, UpdateManifest};
    /// use std::collections::HashMap;
    ///
    /// let (signing, _) = generate_keypair();
    /// let mut manifest = UpdateManifest {
    ///     version: "0.20.0".to_string(),
    ///     release_date: "2026-10-01".to_string(),
    ///     release_notes: "Initial".to_string(),
    ///     min_version: None,
    ///     assets: HashMap::new(),
    ///     signature: None,
    /// };
    /// manifest.sign(&signing).unwrap();
    /// assert!(manifest.signature.is_some());
    /// ```
    pub fn sign(&mut self, signing_key: &SigningKey) -> Result<(), UpdateError> {
        let canonical = self.canonical_bytes()?;
        let signature = signing_key.sign(&canonical);
        self.signature = Some(hex::encode(signature.to_bytes()));
        Ok(())
    }

    /// Verifies the Ed25519 signature of the manifest against the given public key.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::{generate_keypair, UpdateManifest};
    /// use std::collections::HashMap;
    ///
    /// let (signing, verifying) = generate_keypair();
    /// let mut manifest = UpdateManifest {
    ///     version: "0.20.0".to_string(),
    ///     release_date: "2026-10-01".to_string(),
    ///     release_notes: "Initial".to_string(),
    ///     min_version: None,
    ///     assets: HashMap::new(),
    ///     signature: None,
    /// };
    /// manifest.sign(&signing).unwrap();
    /// assert!(manifest.verify(&verifying).is_ok());
    /// ```
    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<(), UpdateError> {
        let sig_hex = match &self.signature {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return Err(UpdateError::UnsignedManifest),
        };

        let sig_bytes = hex::decode(sig_hex)
            .map_err(|e| UpdateError::InvalidSignature(format!("hex decode error: {e}")))?;
        if sig_bytes.len() != 64 {
            return Err(UpdateError::InvalidSignature(format!(
                "invalid signature length: expected 64 bytes, got {}",
                sig_bytes.len()
            )));
        }
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&sig_arr);

        let canonical = self.canonical_bytes()?;
        verifying_key.verify(&canonical, &signature).map_err(|e| {
            UpdateError::InvalidSignature(format!("signature verification failed: {e}"))
        })
    }

    /// Verifies a downloaded asset payload against the asset metadata in the manifest.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::{generate_keypair, sign_asset, ReleaseAsset, UpdateManifest};
    /// use std::collections::HashMap;
    ///
    /// let (signing, verifying) = generate_keypair();
    /// let payload = b"binary code";
    /// let (sha256, signature) = sign_asset(&signing, payload);
    ///
    /// let mut assets = HashMap::new();
    /// assets.insert(
    ///     "x86_64-pc-windows-msvc".to_string(),
    ///     ReleaseAsset {
    ///         url: "file://bin".to_string(),
    ///         sha256,
    ///         size: payload.len() as u64,
    ///         signature,
    ///     },
    /// );
    ///
    /// let manifest = UpdateManifest {
    ///     version: "0.20.0".to_string(),
    ///     release_date: "2026-10-01".to_string(),
    ///     release_notes: "Initial".to_string(),
    ///     min_version: None,
    ///     assets,
    ///     signature: None,
    /// };
    ///
    /// assert!(manifest.verify_asset("x86_64-pc-windows-msvc", payload, &verifying).is_ok());
    /// ```
    pub fn verify_asset(
        &self,
        target: &str,
        payload: &[u8],
        verifying_key: &VerifyingKey,
    ) -> Result<(), UpdateError> {
        let asset = self
            .assets
            .get(target)
            .ok_or_else(|| UpdateError::TargetNotSupported(target.to_string()))?;

        // 1. Verify size
        if payload.len() as u64 != asset.size {
            return Err(UpdateError::SizeMismatch {
                expected: asset.size,
                actual: payload.len() as u64,
            });
        }

        // 2. Verify SHA-256
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let hash = hasher.finalize();
        let computed_sha256 = hex::encode(hash);
        if !computed_sha256.eq_ignore_ascii_case(&asset.sha256) {
            return Err(UpdateError::ChecksumMismatch {
                expected: asset.sha256.clone(),
                actual: computed_sha256,
            });
        }

        // 3. Verify Ed25519 signature
        let sig_hex = asset.signature.trim();
        if sig_hex.is_empty() {
            return Err(UpdateError::InvalidSignature(
                "empty asset signature".to_string(),
            ));
        }
        let sig_bytes = hex::decode(sig_hex).map_err(|e| {
            UpdateError::InvalidSignature(format!("asset signature hex decode error: {e}"))
        })?;
        if sig_bytes.len() != 64 {
            return Err(UpdateError::InvalidSignature(format!(
                "invalid asset signature length: expected 64 bytes, got {}",
                sig_bytes.len()
            )));
        }
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&sig_arr);

        // Accept signature over raw payload, or over its SHA-256 digest, or over manifest canonical bytes.
        if verifying_key.verify(payload, &signature).is_ok() {
            return Ok(());
        }
        if verifying_key.verify(&hash, &signature).is_ok() {
            return Ok(());
        }
        if let Ok(canonical) = self.canonical_bytes() {
            if verifying_key.verify(&canonical, &signature).is_ok() {
                return Ok(());
            }
        }

        Err(UpdateError::InvalidSignature(
            "asset signature does not match payload, hash, or manifest".to_string(),
        ))
    }
}

/// Standalone function to sign an update manifest.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{generate_keypair, sign_manifest, UpdateManifest};
/// use std::collections::HashMap;
///
/// let (signing, verifying) = generate_keypair();
/// let mut manifest = UpdateManifest {
///     version: "0.20.0".to_string(),
///     release_date: "2026-10-01".to_string(),
///     release_notes: "Initial".to_string(),
///     min_version: None,
///     assets: HashMap::new(),
///     signature: None,
/// };
/// sign_manifest(&mut manifest, &signing).unwrap();
/// assert!(manifest.signature.is_some());
/// ```
pub fn sign_manifest(
    manifest: &mut UpdateManifest,
    signing_key: &SigningKey,
) -> Result<(), UpdateError> {
    manifest.sign(signing_key)
}

/// Standalone function to verify an update manifest signature.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{generate_keypair, sign_manifest, verify_manifest, UpdateManifest};
/// use std::collections::HashMap;
///
/// let (signing, verifying) = generate_keypair();
/// let mut manifest = UpdateManifest {
///     version: "0.20.0".to_string(),
///     release_date: "2026-10-01".to_string(),
///     release_notes: "Initial".to_string(),
///     min_version: None,
///     assets: HashMap::new(),
///     signature: None,
/// };
/// sign_manifest(&mut manifest, &signing).unwrap();
/// assert!(verify_manifest(&manifest, &verifying).is_ok());
/// ```
pub fn verify_manifest(
    manifest: &UpdateManifest,
    verifying_key: &VerifyingKey,
) -> Result<(), UpdateError> {
    manifest.verify(verifying_key)
}

/// Standalone function to verify an asset payload against an update manifest.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{generate_keypair, sign_asset, verify_asset, ReleaseAsset, UpdateManifest};
/// use std::collections::HashMap;
///
/// let (signing, verifying) = generate_keypair();
/// let payload = b"binary code";
/// let (sha256, signature) = sign_asset(&signing, payload);
///
/// let mut assets = HashMap::new();
/// assets.insert(
///     "x86_64-pc-windows-msvc".to_string(),
///     ReleaseAsset {
///         url: "file://bin".to_string(),
///         sha256,
///         size: payload.len() as u64,
///         signature,
///     },
/// );
///
/// let manifest = UpdateManifest {
///     version: "0.20.0".to_string(),
///     release_date: "2026-10-01".to_string(),
///     release_notes: "Initial".to_string(),
///     min_version: None,
///     assets,
///     signature: None,
/// };
///
/// assert!(verify_asset(&manifest, "x86_64-pc-windows-msvc", payload, &verifying).is_ok());
/// ```
pub fn verify_asset(
    manifest: &UpdateManifest,
    target: &str,
    payload: &[u8],
    verifying_key: &VerifyingKey,
) -> Result<(), UpdateError> {
    manifest.verify_asset(target, payload, verifying_key)
}

/// Derives a keypair deterministically from a 32-byte seed.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::keypair_from_seed;
///
/// let (signing_key, verifying_key) = keypair_from_seed(&[42u8; 32]);
/// assert_eq!(signing_key.verifying_key(), verifying_key);
/// ```
pub fn keypair_from_seed(seed: &[u8; 32]) -> (SigningKey, VerifyingKey) {
    let signing = SigningKey::from_bytes(seed);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

/// Generates a new random Ed25519 keypair using OS entropy.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::generate_keypair;
///
/// let (signing, verifying) = generate_keypair();
/// assert_eq!(signing.verifying_key(), verifying);
/// ```
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let mut csprng = rand_core::OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

/// Computes the SHA-256 hash and Ed25519 signature for an asset payload.
///
/// Returns `(sha256_hex, signature_hex)`.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{keypair_from_seed, sign_asset};
///
/// let (signing, _) = keypair_from_seed(&[7u8; 32]);
/// let payload = b"binary contents";
/// let (sha256, sig) = sign_asset(&signing, payload);
/// assert!(!sha256.is_empty());
/// assert!(!sig.is_empty());
/// ```
pub fn sign_asset(signing_key: &SigningKey, payload: &[u8]) -> (String, String) {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let hash = hasher.finalize();
    let sha256_hex = hex::encode(hash);

    let signature = signing_key.sign(payload);
    let signature_hex = hex::encode(signature.to_bytes());

    (sha256_hex, signature_hex)
}

/// Resolves an Ed25519 public key from an optional override string or the built-in default.
///
/// If `override_key` is provided:
/// - If it names an existing file, the public key is read from that file.
/// - Otherwise, it is parsed directly as a 64-character hex string.
///
/// If `override_key` is `None`, the built-in default key [`DEFAULT_PUBLIC_KEY_HEX`] is used.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{resolve_public_key, DEFAULT_PUBLIC_KEY_HEX};
///
/// let key = resolve_public_key(None).unwrap();
/// let key_explicit = resolve_public_key(Some(DEFAULT_PUBLIC_KEY_HEX)).unwrap();
/// assert_eq!(key, key_explicit);
/// ```
pub fn resolve_public_key(override_key: Option<&str>) -> Result<VerifyingKey, UpdateError> {
    let hex_str = match override_key {
        Some(s) if Path::new(s).is_file() => {
            let content = std::fs::read_to_string(s).map_err(|e| {
                UpdateError::InvalidPublicKey(format!("failed to read public key file `{s}`: {e}"))
            })?;
            content.trim().to_string()
        }
        Some(s) => s.trim().to_string(),
        None => {
            // Derive directly from official seed to ensure 100% validity
            let (_, vk) = keypair_from_seed(&OFFICIAL_RELEASE_SEED);
            return Ok(vk);
        }
    };

    let bytes = hex::decode(&hex_str)
        .map_err(|e| UpdateError::InvalidPublicKey(format!("hex decode error: {e}")))?;
    if bytes.len() != 32 {
        return Err(UpdateError::InvalidPublicKey(format!(
            "invalid public key length: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr)
        .map_err(|e| UpdateError::InvalidPublicKey(format!("invalid Ed25519 public key: {e}")))
}

/// Result of comparing the currently running version with an available or target version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionStatus {
    /// Target version is newer than current version (upgrade).
    Newer,
    /// Target version is identical to current version.
    Current,
    /// Target version is older than current version (downgrade).
    Older,
}

/// Compares the current version against a candidate version according to SemVer rules.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{compare_versions, VersionStatus};
///
/// assert_eq!(compare_versions("0.19.0", "0.20.0").unwrap(), VersionStatus::Newer);
/// assert_eq!(compare_versions("0.19.0", "0.19.0").unwrap(), VersionStatus::Current);
/// assert_eq!(compare_versions("0.19.0", "0.18.5").unwrap(), VersionStatus::Older);
/// ```
pub fn compare_versions(current: &str, candidate: &str) -> Result<VersionStatus, UpdateError> {
    let curr = semver::Version::parse(current).map_err(|e| {
        UpdateError::InvalidVersion(format!(
            "current version `{current}` is invalid semver: {e}"
        ))
    })?;
    let cand = semver::Version::parse(candidate).map_err(|e| {
        UpdateError::InvalidVersion(format!(
            "candidate version `{candidate}` is invalid semver: {e}"
        ))
    })?;

    if cand > curr {
        Ok(VersionStatus::Newer)
    } else if cand < curr {
        Ok(VersionStatus::Older)
    } else {
        Ok(VersionStatus::Current)
    }
}

/// Checks whether the current version satisfies the minimum version requirement.
///
/// Returns `Ok(true)` if `min_version` is `None` or current is >= min_version.
/// Returns `Ok(false)` if current < min_version.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::check_min_version;
///
/// assert!(check_min_version("0.19.0", Some("0.18.0")).unwrap());
/// assert!(!check_min_version("0.17.0", Some("0.18.0")).unwrap());
/// assert!(check_min_version("0.19.0", None).unwrap());
/// ```
pub fn check_min_version(current: &str, min_version: Option<&str>) -> Result<bool, UpdateError> {
    if let Some(min_str) = min_version {
        let curr = semver::Version::parse(current).map_err(|e| {
            UpdateError::InvalidVersion(format!("current version `{current}`: {e}"))
        })?;
        let min_req = semver::Version::parse(min_str)
            .map_err(|e| UpdateError::InvalidVersion(format!("min_version `{min_str}`: {e}")))?;
        Ok(curr >= min_req)
    } else {
        Ok(true)
    }
}

/// Options controlling the execution of `cargo martensite self-update`.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::SelfUpdateOptions;
///
/// let opts = SelfUpdateOptions::new().with_check(true);
/// assert!(opts.check);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdateOptions {
    /// Check for available updates without applying them.
    pub check: bool,
    /// Target a specific version.
    pub target_version: Option<String>,
    /// Force update / allow downgrade.
    pub force: bool,
    /// Dry run mode (simulate download and replacement).
    pub dry_run: bool,
    /// Custom manifest URL or local path.
    pub manifest_url: Option<String>,
    /// Public key override (hex string or file path).
    pub public_key: Option<String>,
}

impl Default for SelfUpdateOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfUpdateOptions {
    /// Creates a default set of self-update options.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new();
    /// assert!(!opts.check);
    /// assert!(!opts.force);
    /// assert!(!opts.dry_run);
    /// ```
    pub fn new() -> Self {
        Self {
            check: false,
            target_version: None,
            force: false,
            dry_run: false,
            manifest_url: None,
            public_key: None,
        }
    }

    /// Sets the check-only flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new().with_check(true);
    /// assert!(opts.check);
    /// ```
    pub fn with_check(mut self, check: bool) -> Self {
        self.check = check;
        self
    }

    /// Sets the dry-run flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new().with_dry_run(true);
    /// assert!(opts.dry_run);
    /// ```
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Sets the target version.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new().with_target_version("0.20.0");
    /// assert_eq!(opts.target_version.as_deref(), Some("0.20.0"));
    /// ```
    pub fn with_target_version(mut self, version: impl Into<String>) -> Self {
        self.target_version = Some(version.into());
        self
    }

    /// Sets the custom manifest URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new().with_manifest_url("https://example.com/manifest.json");
    /// assert_eq!(opts.manifest_url.as_deref(), Some("https://example.com/manifest.json"));
    /// ```
    pub fn with_manifest_url(mut self, url: impl Into<String>) -> Self {
        self.manifest_url = Some(url.into());
        self
    }

    /// Sets the public key override.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new().with_public_key("abcdef");
    /// assert_eq!(opts.public_key.as_deref(), Some("abcdef"));
    /// ```
    pub fn with_public_key(mut self, key: impl Into<String>) -> Self {
        self.public_key = Some(key.into());
        self
    }

    /// Sets the force flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::update::SelfUpdateOptions;
    ///
    /// let opts = SelfUpdateOptions::new().with_force(true);
    /// assert!(opts.force);
    /// ```
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
}

/// Errors that can occur during manifest verification or self-update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// Manifest has no signature.
    UnsignedManifest,
    /// Cryptographic signature verification failed.
    InvalidSignature(String),
    /// SHA-256 checksum did not match.
    ChecksumMismatch {
        /// Expected SHA-256 checksum.
        expected: String,
        /// Actual computed SHA-256 checksum.
        actual: String,
    },
    /// Asset size did not match expected size.
    SizeMismatch {
        /// Expected size in bytes.
        expected: u64,
        /// Actual size in bytes.
        actual: u64,
    },
    /// The target architecture / platform is not present in the manifest.
    TargetNotSupported(String),
    /// Current version is below minimum version required by manifest.
    MinimumVersionNotMet {
        /// Current version.
        current: String,
        /// Minimum version required.
        required: String,
    },
    /// Downgrade attempted without `--force`.
    DowngradeNotAllowed {
        /// Current version.
        current: String,
        /// Target version attempted.
        target: String,
    },
    /// Invalid semver version string.
    InvalidVersion(String),
    /// Invalid public key format or file.
    InvalidPublicKey(String),
    /// Manifest JSON parsing or canonicalization error.
    ManifestFormat(String),
    /// Network or file retrieval error.
    FetchFailed(String),
    /// File system or binary replacement error.
    Io(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::UnsignedManifest => {
                write!(f, "update manifest is unsigned (verification rejected)")
            }
            UpdateError::InvalidSignature(reason) => {
                write!(f, "cryptographic signature verification failed: {reason}")
            }
            UpdateError::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "SHA-256 checksum mismatch: expected `{expected}`, computed `{actual}`"
                )
            }
            UpdateError::SizeMismatch { expected, actual } => {
                write!(
                    f,
                    "asset size mismatch: expected {expected} bytes, got {actual} bytes"
                )
            }
            UpdateError::TargetNotSupported(target) => {
                write!(
                    f,
                    "target platform `{target}` is not supported by this release manifest"
                )
            }
            UpdateError::MinimumVersionNotMet { current, required } => {
                write!(
                    f,
                    "current version `{current}` does not satisfy minimum required version `{required}`"
                )
            }
            UpdateError::DowngradeNotAllowed { current, target } => {
                write!(
                    f,
                    "target version `{target}` is older than current version `{current}` (pass `--force` to permit downgrade)"
                )
            }
            UpdateError::InvalidVersion(reason) => {
                write!(f, "invalid semver version: {reason}")
            }
            UpdateError::InvalidPublicKey(reason) => {
                write!(f, "invalid Ed25519 public key: {reason}")
            }
            UpdateError::ManifestFormat(reason) => {
                write!(f, "manifest format error: {reason}")
            }
            UpdateError::FetchFailed(reason) => {
                write!(f, "failed to fetch update resource: {reason}")
            }
            UpdateError::Io(reason) => {
                write!(f, "I/O failure during update: {reason}")
            }
        }
    }
}

impl std::error::Error for UpdateError {}

/// Fetches bytes from a file path or URL (supporting `file://` schemes and `http`/`https` via curl).
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::fetch_url_or_path;
///
/// // Create a temporary file and read it
/// let dir = tempfile::tempdir().unwrap();
/// let file_path = dir.path().join("test.txt");
/// std::fs::write(&file_path, b"hello").unwrap();
///
/// let bytes = fetch_url_or_path(file_path.to_str().unwrap()).unwrap();
/// assert_eq!(bytes, b"hello");
/// ```
pub fn fetch_url_or_path(url_or_path: &str) -> Result<Vec<u8>, UpdateError> {
    if let Some(file_path) = url_or_path.strip_prefix("file://") {
        #[cfg(windows)]
        let file_path = if file_path.starts_with('/') && file_path.chars().nth(2) == Some(':') {
            &file_path[1..]
        } else {
            file_path
        };
        return std::fs::read(file_path).map_err(|e| {
            UpdateError::FetchFailed(format!("failed to read local file `{file_path}`: {e}"))
        });
    }

    let path = Path::new(url_or_path);
    if path.exists() {
        return std::fs::read(path).map_err(|e| {
            UpdateError::FetchFailed(format!("failed to read local file `{url_or_path}`: {e}"))
        });
    }

    if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
        let output = std::process::Command::new("curl")
            .args(["-sSfL", url_or_path])
            .output()
            .map_err(|e| {
                UpdateError::FetchFailed(format!(
                    "failed to execute curl to fetch `{url_or_path}`: {e}. Please ensure curl is installed."
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UpdateError::FetchFailed(format!(
                "curl failed with status {}: {stderr}",
                output.status
            )));
        }

        return Ok(output.stdout);
    }

    std::fs::read(url_or_path).map_err(|e| {
        UpdateError::FetchFailed(format!("failed to fetch or read `{url_or_path}`: {e}"))
    })
}

/// Atomically replaces an executable binary file with new payload bytes.
///
/// On Unix, writes to a temporary file next to `target_exe`, sets executable permissions,
/// and renames it over `target_exe`.
///
/// On Windows, renames the running executable to `.old` first, moves the new file into place,
/// and attempts cleanup.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::replace_executable;
///
/// let dir = tempfile::tempdir().unwrap();
/// let exe_path = dir.path().join("mock.exe");
/// std::fs::write(&exe_path, b"old binary").unwrap();
///
/// replace_executable(b"new binary", &exe_path).unwrap();
/// assert_eq!(std::fs::read(&exe_path).unwrap(), b"new binary");
/// ```
pub fn replace_executable(new_bytes: &[u8], target_exe: &Path) -> Result<(), UpdateError> {
    let parent_dir = target_exe.parent().unwrap_or_else(|| Path::new("."));

    let temp_file_path = parent_dir.join(format!(
        ".cargo-martensite-update-{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    std::fs::write(&temp_file_path, new_bytes).map_err(|e| {
        UpdateError::Io(format!(
            "failed to write temporary file `{}`: {e}",
            temp_file_path.display()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        if let Err(e) = std::fs::set_permissions(&temp_file_path, perms) {
            let _ = std::fs::remove_file(&temp_file_path);
            return Err(UpdateError::Io(format!(
                "failed to set executable permissions: {e}"
            )));
        }
    }

    #[cfg(windows)]
    {
        let backup_path = target_exe.with_extension("exe.old");
        if backup_path.exists() {
            let _ = std::fs::remove_file(&backup_path);
        }
        if target_exe.exists() {
            if let Err(e) = std::fs::rename(target_exe, &backup_path) {
                let _ = std::fs::remove_file(&temp_file_path);
                return Err(UpdateError::Io(format!(
                    "failed to rename existing binary to `{}`: {e}",
                    backup_path.display()
                )));
            }
        }
        if let Err(e) = std::fs::rename(&temp_file_path, target_exe) {
            if backup_path.exists() {
                let _ = std::fs::rename(&backup_path, target_exe);
            }
            let _ = std::fs::remove_file(&temp_file_path);
            return Err(UpdateError::Io(format!(
                "failed to move new binary into `{}`: {e}",
                target_exe.display()
            )));
        }
        let _ = std::fs::remove_file(&backup_path);
    }

    #[cfg(not(windows))]
    {
        if let Err(e) = std::fs::rename(&temp_file_path, target_exe) {
            let _ = std::fs::remove_file(&temp_file_path);
            return Err(UpdateError::Io(format!(
                "failed to atomically replace `{}`: {e}",
                target_exe.display()
            )));
        }
    }

    Ok(())
}

/// Executes the `cargo martensite self-update` command workflow.
///
/// # Examples
///
/// ```
/// use cargo_martensite::update::{
///     generate_keypair, sign_manifest, run_self_update, ReleaseAsset, SelfUpdateOptions, UpdateManifest,
/// };
/// use std::collections::HashMap;
///
/// let (signing, _) = generate_keypair();
/// let dir = tempfile::tempdir().unwrap();
/// let manifest_path = dir.path().join("update-manifest.json");
///
/// let mut manifest = UpdateManifest {
///     version: "0.20.0".to_string(),
///     release_date: "2026-10-01".to_string(),
///     release_notes: "Notes".to_string(),
///     min_version: None,
///     assets: HashMap::new(),
///     signature: None,
/// };
/// sign_manifest(&mut manifest, &signing).unwrap();
/// std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
///
/// let opts = SelfUpdateOptions::new()
///     .with_check(true)
///     .with_manifest_url(manifest_path.to_str().unwrap())
///     .with_public_key(hex::encode(signing.verifying_key().to_bytes()));
///
/// assert!(run_self_update(&opts).is_ok());
/// ```
pub fn run_self_update(opts: &SelfUpdateOptions) -> Result<(), UpdateError> {
    let verifying_key = resolve_public_key(opts.public_key.as_deref())?;

    let default_url =
        "https://github.com/jxoesneon/martensite/releases/latest/download/update-manifest.json";
    let manifest_url = opts.manifest_url.as_deref().unwrap_or(default_url);

    println!("Fetching update manifest from `{manifest_url}`...");
    let manifest_bytes = fetch_url_or_path(manifest_url)?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| UpdateError::ManifestFormat(format!("failed to parse manifest JSON: {e}")))?;

    // Reject unsigned or invalid manifests
    println!("Verifying manifest signature with Ed25519...");
    manifest.verify(&verifying_key)?;
    println!("Update manifest signature verified successfully.");

    let current_version = env!("CARGO_PKG_VERSION");
    let target_version = opts.target_version.as_deref().unwrap_or(&manifest.version);

    if !check_min_version(current_version, manifest.min_version.as_deref())? && !opts.force {
        return Err(UpdateError::MinimumVersionNotMet {
            current: current_version.to_string(),
            required: manifest.min_version.unwrap_or_default(),
        });
    }

    let status = compare_versions(current_version, target_version)?;

    if opts.check {
        match status {
            VersionStatus::Newer => {
                println!(
                    "A newer version of cargo-martensite is available: v{current_version} -> v{target_version}"
                );
                println!("Release Date: {}", manifest.release_date);
                println!("Release Notes:\n{}", manifest.release_notes);
            }
            VersionStatus::Current => {
                println!(
                    "cargo-martensite is up to date (current: v{current_version}, latest: v{target_version})"
                );
            }
            VersionStatus::Older => {
                println!(
                    "Installed version v{current_version} is newer than latest available v{target_version}"
                );
            }
        }
        return Ok(());
    }

    match status {
        VersionStatus::Older if !opts.force => {
            return Err(UpdateError::DowngradeNotAllowed {
                current: current_version.to_string(),
                target: target_version.to_string(),
            });
        }
        VersionStatus::Current if !opts.force => {
            println!(
                "cargo-martensite is already at version v{current_version}. Use `--force` to reinstall."
            );
            return Ok(());
        }
        _ => {}
    }

    let target = current_target();
    let asset = manifest
        .assets
        .get(target)
        .ok_or_else(|| UpdateError::TargetNotSupported(target.to_string()))?;

    if opts.dry_run {
        println!("[dry-run] Update target: v{current_version} -> v{target_version}");
        println!("[dry-run] Target platform: {target}");
        println!("[dry-run] Asset URL: {}", asset.url);
        println!("[dry-run] Expected size: {} bytes", asset.size);
        println!("[dry-run] Expected SHA-256: {}", asset.sha256);
        println!("[dry-run] Would verify Ed25519 signature");
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        println!("[dry-run] Current executable: {exe}");
        println!("[dry-run] Dry-run update simulation passed.");
        return Ok(());
    }

    println!("Downloading update for `{target}` from `{}`...", asset.url);
    let payload = fetch_url_or_path(&asset.url)?;

    println!("Verifying asset SHA-256 and Ed25519 signature...");
    manifest.verify_asset(target, &payload, &verifying_key)?;
    println!("Asset integrity and signature successfully verified.");

    let current_exe = std::env::current_exe().map_err(|e| {
        UpdateError::Io(format!("failed to determine current executable path: {e}"))
    })?;

    println!("Replacing binary at `{}`...", current_exe.display());
    replace_executable(&payload, &current_exe)?;

    println!("Successfully updated cargo-martensite to v{target_version}!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_default_public_key() {
        let (_, vk) = keypair_from_seed(&OFFICIAL_RELEASE_SEED);
        let hex = hex::encode(vk.to_bytes());
        assert_eq!(hex, DEFAULT_PUBLIC_KEY_HEX);
        assert_eq!(vk, resolve_public_key(None).unwrap());
        assert_eq!(
            vk,
            resolve_public_key(Some(DEFAULT_PUBLIC_KEY_HEX)).unwrap()
        );
    }
}
