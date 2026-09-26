//! Integration tests for Ed25519 signed update manifests and self-update command.

use cargo_martensite::update::{
    check_min_version, compare_versions, current_target, generate_keypair, keypair_from_seed,
    replace_executable, resolve_public_key, run_self_update, sign_asset, sign_manifest,
    verify_asset, verify_manifest, ReleaseAsset, SelfUpdateOptions, UpdateError, UpdateManifest,
    VersionStatus, CURRENT_TARGET, DEFAULT_PUBLIC_KEY_HEX, OFFICIAL_RELEASE_SEED,
};
use std::collections::HashMap;

#[test]
fn test_manifest_serialization_and_deserialization() {
    let mut assets = HashMap::new();
    assets.insert(
        "x86_64-pc-windows-msvc".to_string(),
        ReleaseAsset {
            url: "https://github.com/jxoesneon/martensite/releases/download/v0.20.0/cargo-martensite-x86_64-windows.zip".to_string(),
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            size: 15482910,
            signature: "a".repeat(128),
        },
    );
    assets.insert(
        "x86_64-unknown-linux-gnu".to_string(),
        ReleaseAsset {
            url: "https://github.com/jxoesneon/martensite/releases/download/v0.20.0/cargo-martensite-x86_64-linux.tar.gz".to_string(),
            sha256: "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce".to_string(),
            size: 14209182,
            signature: "b".repeat(128),
        },
    );

    let original = UpdateManifest {
        version: "0.20.0".to_string(),
        release_date: "2026-10-01".to_string(),
        release_notes: "## Changes\n- Ed25519 signed updates\n- Performance improvements"
            .to_string(),
        min_version: Some("0.18.0".to_string()),
        assets,
        signature: Some("c".repeat(128)),
    };

    let serialized = serde_json::to_string_pretty(&original).expect("serialization failed");
    let deserialized: UpdateManifest =
        serde_json::from_str(&serialized).expect("deserialization failed");

    assert_eq!(original, deserialized);
    assert_eq!(deserialized.version, "0.20.0");
    assert_eq!(deserialized.assets.len(), 2);
    assert_eq!(deserialized.min_version.as_deref(), Some("0.18.0"));

    // Verify canonical serialization is deterministic
    let bytes1 = original.canonical_bytes().unwrap();
    let bytes2 = deserialized.canonical_bytes().unwrap();
    assert_eq!(bytes1, bytes2);
}

#[test]
fn test_ed25519_keypair_generation_signing_and_verification() {
    let (signing_key, verifying_key) = generate_keypair();
    assert_eq!(signing_key.verifying_key(), verifying_key);

    let payload = b"Martensite binary executable payload v0.20.0";
    let (sha256, asset_sig) = sign_asset(&signing_key, payload);
    assert_eq!(sha256.len(), 64);
    assert_eq!(asset_sig.len(), 128);

    let mut assets = HashMap::new();
    assets.insert(
        CURRENT_TARGET.to_string(),
        ReleaseAsset {
            url: "file://target-binary".to_string(),
            sha256: sha256.clone(),
            size: payload.len() as u64,
            signature: asset_sig.clone(),
        },
    );

    let mut manifest = UpdateManifest {
        version: "0.20.0".to_string(),
        release_date: "2026-10-01".to_string(),
        release_notes: "Signed release test".to_string(),
        min_version: None,
        assets,
        signature: None,
    };

    // Sign manifest
    sign_manifest(&mut manifest, &signing_key).expect("manifest signing failed");
    assert!(manifest.signature.is_some());

    // Verify manifest
    verify_manifest(&manifest, &verifying_key).expect("manifest verification failed");

    // Verify asset
    verify_asset(&manifest, CURRENT_TARGET, payload, &verifying_key)
        .expect("asset verification failed");
}

#[test]
fn test_deterministic_keypair_from_seed() {
    let seed1 = [0x5au8; 32];
    let (sk1, vk1) = keypair_from_seed(&seed1);
    let (sk2, vk2) = keypair_from_seed(&seed1);
    assert_eq!(sk1.to_bytes(), sk2.to_bytes());
    assert_eq!(vk1.to_bytes(), vk2.to_bytes());

    let (official_sk, official_vk) = keypair_from_seed(&OFFICIAL_RELEASE_SEED);
    let official_hex = hex::encode(official_vk.to_bytes());
    assert_eq!(official_hex, DEFAULT_PUBLIC_KEY_HEX);
    assert_eq!(official_sk.verifying_key(), official_vk);
}

#[test]
fn test_tamper_detection_manifest_modification_fails() {
    let (signing_key, verifying_key) = generate_keypair();
    let payload = b"Original binary payload";
    let (sha256, asset_sig) = sign_asset(&signing_key, payload);

    let mut assets = HashMap::new();
    assets.insert(
        CURRENT_TARGET.to_string(),
        ReleaseAsset {
            url: "https://example.com/bin".to_string(),
            sha256,
            size: payload.len() as u64,
            signature: asset_sig,
        },
    );

    let mut valid_manifest = UpdateManifest {
        version: "0.20.0".to_string(),
        release_date: "2026-10-01".to_string(),
        release_notes: "Release notes".to_string(),
        min_version: Some("0.18.0".to_string()),
        assets,
        signature: None,
    };
    sign_manifest(&mut valid_manifest, &signing_key).unwrap();

    // 1. Modifying 1 char in version fails verification
    let mut tampered = valid_manifest.clone();
    tampered.version = "0.20.1".to_string();
    let err = verify_manifest(&tampered, &verifying_key).unwrap_err();
    assert!(matches!(err, UpdateError::InvalidSignature(_)));

    // 2. Modifying release date fails
    let mut tampered = valid_manifest.clone();
    tampered.release_date = "2026-10-02".to_string();
    assert!(verify_manifest(&tampered, &verifying_key).is_err());

    // 3. Modifying release notes fails
    let mut tampered = valid_manifest.clone();
    tampered.release_notes = "Tampered notes".to_string();
    assert!(verify_manifest(&tampered, &verifying_key).is_err());

    // 4. Modifying asset sha256 in manifest fails
    let mut tampered = valid_manifest.clone();
    let asset = tampered.assets.get_mut(CURRENT_TARGET).unwrap();
    asset.sha256 = "0".repeat(64);
    assert!(verify_manifest(&tampered, &verifying_key).is_err());

    // 5. Modifying asset URL fails
    let mut tampered = valid_manifest.clone();
    let asset = tampered.assets.get_mut(CURRENT_TARGET).unwrap();
    asset.url = "https://evil.com/bin".to_string();
    assert!(verify_manifest(&tampered, &verifying_key).is_err());

    // 6. Flipping 1 bit of signature fails
    let mut tampered = valid_manifest.clone();
    let sig = tampered.signature.as_mut().unwrap();
    let last_char = if sig.ends_with('0') { '1' } else { '0' };
    sig.pop();
    sig.push(last_char);
    assert!(verify_manifest(&tampered, &verifying_key).is_err());

    // 7. Unsigned manifest is rejected
    let mut unsigned = valid_manifest.clone();
    unsigned.signature = None;
    let err = verify_manifest(&unsigned, &verifying_key).unwrap_err();
    assert_eq!(err, UpdateError::UnsignedManifest);
}

#[test]
fn test_tamper_detection_asset_modification_fails() {
    let (signing_key, verifying_key) = generate_keypair();
    let payload = b"Authentic production binary bytes";
    let (sha256, asset_sig) = sign_asset(&signing_key, payload);

    let mut assets = HashMap::new();
    assets.insert(
        CURRENT_TARGET.to_string(),
        ReleaseAsset {
            url: "https://example.com/bin".to_string(),
            sha256,
            size: payload.len() as u64,
            signature: asset_sig,
        },
    );

    let mut manifest = UpdateManifest {
        version: "0.20.0".to_string(),
        release_date: "2026-10-01".to_string(),
        release_notes: "Notes".to_string(),
        min_version: None,
        assets,
        signature: None,
    };
    sign_manifest(&mut manifest, &signing_key).unwrap();

    // 1. Modifying 1 bit of the payload triggers ChecksumMismatch or InvalidSignature
    let mut tampered_payload = payload.to_vec();
    tampered_payload[0] ^= 0x01; // flip 1 bit
    let err =
        verify_asset(&manifest, CURRENT_TARGET, &tampered_payload, &verifying_key).unwrap_err();
    assert!(matches!(err, UpdateError::ChecksumMismatch { .. }));

    // 2. Modifying payload size triggers SizeMismatch
    let mut truncated_payload = payload.to_vec();
    truncated_payload.pop();
    let err = verify_asset(
        &manifest,
        CURRENT_TARGET,
        &truncated_payload,
        &verifying_key,
    )
    .unwrap_err();
    assert!(matches!(err, UpdateError::SizeMismatch { .. }));

    // 3. Modifying 1 bit of asset signature triggers InvalidSignature
    let mut tampered_manifest = manifest.clone();
    let asset = tampered_manifest.assets.get_mut(CURRENT_TARGET).unwrap();
    let last_char = if asset.signature.ends_with('0') {
        '1'
    } else {
        '0'
    };
    asset.signature.pop();
    asset.signature.push(last_char);
    let err =
        verify_asset(&tampered_manifest, CURRENT_TARGET, payload, &verifying_key).unwrap_err();
    assert!(matches!(err, UpdateError::InvalidSignature(_)));
}

#[test]
fn test_version_comparison_semver_logic() {
    assert_eq!(
        compare_versions("0.19.0", "0.20.0").unwrap(),
        VersionStatus::Newer
    );
    assert_eq!(
        compare_versions("0.19.0", "1.0.0").unwrap(),
        VersionStatus::Newer
    );
    assert_eq!(
        compare_versions("0.19.0", "0.19.1").unwrap(),
        VersionStatus::Newer
    );
    assert_eq!(
        compare_versions("0.19.0", "0.19.0").unwrap(),
        VersionStatus::Current
    );
    assert_eq!(
        compare_versions("0.19.0", "0.18.9").unwrap(),
        VersionStatus::Older
    );
    assert_eq!(
        compare_versions("0.19.0", "0.1.0").unwrap(),
        VersionStatus::Older
    );

    // Pre-release comparisons
    assert_eq!(
        compare_versions("0.19.0-rc.1", "0.19.0").unwrap(),
        VersionStatus::Newer
    );
    assert_eq!(
        compare_versions("0.19.0", "0.19.0-rc.1").unwrap(),
        VersionStatus::Older
    );

    // Invalid version strings
    assert!(compare_versions("invalid", "0.20.0").is_err());
    assert!(compare_versions("0.19.0", "not-semver").is_err());

    // Minimum version checks
    assert!(check_min_version("0.19.0", Some("0.18.0")).unwrap());
    assert!(check_min_version("0.19.0", Some("0.19.0")).unwrap());
    assert!(!check_min_version("0.17.0", Some("0.18.0")).unwrap());
    assert!(check_min_version("0.19.0", None).unwrap());
    assert!(check_min_version("0.19.0", Some("invalid")).is_err());
}

#[test]
fn test_public_key_resolution_and_overrides() {
    // 1. Default built-in key resolution
    let default_key = resolve_public_key(None).expect("default public key must resolve");
    assert_eq!(hex::encode(default_key.to_bytes()), DEFAULT_PUBLIC_KEY_HEX);

    // 2. Explicit hex override
    let (_signing, verifying) = generate_keypair();
    let hex_key = hex::encode(verifying.to_bytes());
    let resolved = resolve_public_key(Some(&hex_key)).expect("hex key resolution failed");
    assert_eq!(verifying, resolved);

    // 3. File path override
    let dir = tempfile::tempdir().unwrap();
    let key_file = dir.path().join("martensite.pub");
    std::fs::write(&key_file, format!("  {hex_key}\n  ")).unwrap();
    let resolved_from_file =
        resolve_public_key(Some(key_file.to_str().unwrap())).expect("file key resolution failed");
    assert_eq!(verifying, resolved_from_file);

    // 4. Invalid hex / length error
    assert!(resolve_public_key(Some("1234")).is_err());
    assert!(resolve_public_key(Some("not-valid-hex-characters-here-length32!!")).is_err());
}

#[test]
fn test_mock_update_dry_run_workflow() {
    let (signing_key, verifying_key) = generate_keypair();
    let payload = b"Mock new binary content for self-update dry-run";
    let (sha256, asset_sig) = sign_asset(&signing_key, payload);

    let dir = tempfile::tempdir().unwrap();
    let asset_file = dir.path().join("mock-cargo-martensite.bin");
    std::fs::write(&asset_file, payload).unwrap();

    let manifest_file = dir.path().join("update-manifest.json");

    let mut assets = HashMap::new();
    assets.insert(
        CURRENT_TARGET.to_string(),
        ReleaseAsset {
            url: format!("file://{}", asset_file.to_str().unwrap()),
            sha256,
            size: payload.len() as u64,
            signature: asset_sig,
        },
    );

    let mut manifest = UpdateManifest {
        version: "0.20.0".to_string(),
        release_date: "2026-10-01".to_string(),
        release_notes: "v0.20.0 release notes".to_string(),
        min_version: Some("0.18.0".to_string()),
        assets,
        signature: None,
    };
    sign_manifest(&mut manifest, &signing_key).unwrap();

    let manifest_json = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(&manifest_file, manifest_json).unwrap();

    let pubkey_hex = hex::encode(verifying_key.to_bytes());

    // Test --dry-run
    let opts = SelfUpdateOptions::new()
        .with_dry_run(true)
        .with_manifest_url(manifest_file.to_str().unwrap())
        .with_public_key(&pubkey_hex);

    let result = run_self_update(&opts);
    assert!(
        result.is_ok(),
        "dry-run self-update failed: {:?}",
        result.err()
    );

    // Test --check flag
    let check_opts = SelfUpdateOptions::new()
        .with_check(true)
        .with_manifest_url(manifest_file.to_str().unwrap())
        .with_public_key(&pubkey_hex);

    let check_result = run_self_update(&check_opts);
    assert!(
        check_result.is_ok(),
        "check-only self-update failed: {:?}",
        check_result.err()
    );
}

#[test]
fn test_downgrade_protection_and_force() {
    let (signing_key, verifying_key) = generate_keypair();
    let dir = tempfile::tempdir().unwrap();
    let manifest_file = dir.path().join("update-manifest.json");

    let mut assets = HashMap::new();
    assets.insert(
        CURRENT_TARGET.to_string(),
        ReleaseAsset {
            url: "file://dummy".to_string(),
            sha256: "0".repeat(64),
            size: 0,
            signature: "0".repeat(128),
        },
    );

    let mut manifest = UpdateManifest {
        version: "0.1.0".to_string(), // older than current 0.19.0
        release_date: "2025-01-01".to_string(),
        release_notes: "Ancient version".to_string(),
        min_version: None,
        assets,
        signature: None,
    };
    sign_manifest(&mut manifest, &signing_key).unwrap();
    std::fs::write(&manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let pubkey_hex = hex::encode(verifying_key.to_bytes());

    // Without --force, downgrade must be rejected
    let opts = SelfUpdateOptions::new()
        .with_manifest_url(manifest_file.to_str().unwrap())
        .with_public_key(&pubkey_hex);
    let err = run_self_update(&opts).unwrap_err();
    assert!(matches!(err, UpdateError::DowngradeNotAllowed { .. }));

    // With --force in dry-run mode, downgrade is allowed
    let forced_opts = SelfUpdateOptions::new()
        .with_manifest_url(manifest_file.to_str().unwrap())
        .with_public_key(&pubkey_hex)
        .with_force(true)
        .with_dry_run(true);
    assert!(run_self_update(&forced_opts).is_ok());
}

#[test]
fn test_atomic_executable_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let exe_path = dir.path().join("cargo-martensite-mock.exe");
    std::fs::write(&exe_path, b"original-v0.19.0-binary").unwrap();

    let new_payload = b"updated-v0.20.0-binary-content";
    replace_executable(new_payload, &exe_path).expect("executable replacement failed");

    let updated = std::fs::read(&exe_path).expect("failed to read replaced executable");
    assert_eq!(updated, new_payload);
}

#[test]
fn test_unsupported_target_rejection() {
    let (signing_key, verifying_key) = generate_keypair();
    let mut manifest = UpdateManifest {
        version: "0.20.0".to_string(),
        release_date: "2026-10-01".to_string(),
        release_notes: "Target test".to_string(),
        min_version: None,
        assets: HashMap::new(), // no assets at all
        signature: None,
    };
    sign_manifest(&mut manifest, &signing_key).unwrap();

    let err = verify_asset(&manifest, CURRENT_TARGET, b"test", &verifying_key).unwrap_err();
    assert!(matches!(err, UpdateError::TargetNotSupported(_)));
}

#[test]
fn test_current_target_returns_valid_platform() {
    let target = current_target();
    assert!(!target.is_empty());
    assert_eq!(target, CURRENT_TARGET);
}
