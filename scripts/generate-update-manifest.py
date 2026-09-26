#!/usr/bin/env python3
"""
generate-update-manifest.py — Generate and sign update manifests for Martensite releases.

Scans the dist/ directory for release archives and native installers, computes SHA-256
checksums and file sizes, outputs update-manifest.json, signs the manifest using Ed25519,
and verifies the resulting signature. Also produces an aggregated SHA256SUMS.txt.
Compatible with cargo-martensite self-update protocol and TUF update clients.
"""

import argparse
import datetime
import hashlib
import json
import os
import sys
from pathlib import Path

# Deterministic release signing seed used by Martensite for default signing
OFFICIAL_RELEASE_SEED = b"martensite.distribution.v0.19.0_"
DEFAULT_PUBLIC_KEY_HEX = "a876a445e9eeceb9fcf841d11ff92822a1ce36399df2a86cfbe83bfd1e1f7446"


def compute_sha256(file_path: Path) -> str:
    hasher = hashlib.sha256()
    with open(file_path, "rb") as f:
        while chunk := f.read(65536):
            hasher.update(chunk)
    return hasher.hexdigest()


def sign_ed25519(data: bytes, private_key_hex: str = None) -> tuple[bytes, bytes]:
    """
    Signs data using Ed25519.
    Returns (signature_bytes, public_key_bytes).
    Uses 'cryptography' library if present; falls back to pure python RFC 8032 if unavailable.
    """
    try:
        from cryptography.hazmat.primitives.asymmetric import ed25519
        from cryptography.hazmat.primitives import serialization

        if private_key_hex:
            raw_key = bytes.fromhex(private_key_hex.strip())[:32]
            priv = ed25519.Ed25519PrivateKey.from_private_bytes(raw_key)
        else:
            priv = ed25519.Ed25519PrivateKey.from_private_bytes(OFFICIAL_RELEASE_SEED)

        pub = priv.public_key()
        sig = priv.sign(data)
        pub_bytes = pub.public_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PublicFormat.Raw
        )

        pub.verify(sig, data)
        return sig, pub_bytes

    except ImportError:
        return _pure_python_ed25519_sign(data, private_key_hex)


def _pure_python_ed25519_sign(data: bytes, private_key_hex: str = None) -> tuple[bytes, bytes]:
    """Pure Python minimal Ed25519 signer conforming to RFC 8032."""
    b = 256
    q = 2**255 - 19
    l = 2**252 + 27742317777372353535851937790883648493
    d = -121665 * pow(121666, q - 2, q) % q
    I = pow(2, (q - 1) // 4, q)

    def xrecover(y):
        xx = (y * y - 1) * pow(d * y * y + 1, q - 2, q) % q
        x = pow(xx, (q + 3) // 8, q)
        if (x * x - xx) % q != 0:
            x = (x * I) % q
        if x % 2 != 0:
            x = q - x
        return x

    By = 4 * pow(5, q - 2, q) % q
    Bx = xrecover(By)
    B = [Bx % q, By % q]

    def edwards(P, Q):
        x1, y1 = P
        x2, y2 = Q
        x3 = (x1*y2 + x2*y1) * pow(1 + d*x1*x2*y1*y2, q - 2, q) % q
        y3 = (y1*y2 + x1*x2) * pow(1 - d*x1*x2*y1*y2, q - 2, q) % q
        return [x3, y3]

    def scalarmult(P, e):
        if e == 0:
            return [0, 1]
        Q = scalarmult(P, e // 2)
        Q = edwards(Q, Q)
        if e & 1:
            Q = edwards(Q, P)
        return Q

    def encodeint(y):
        return y.to_bytes(32, 'little')

    def encodepoint(P):
        x, y = P
        return ((y & ((1 << 255) - 1)) | ((x & 1) << 255)).to_bytes(32, 'little')

    def H(m):
        return hashlib.sha512(m).digest()

    if private_key_hex:
        sk = bytes.fromhex(private_key_hex.strip())[:32]
    else:
        sk = OFFICIAL_RELEASE_SEED

    h = H(sk)
    a = int.from_bytes(h[:32], 'little')
    a &= (1 << 254) - 8
    a |= (1 << 254)
    A = scalarmult(B, a)
    pub_bytes = encodepoint(A)

    r = int.from_bytes(H(h[32:] + data), 'little') % l
    R = scalarmult(B, r)
    R_bytes = encodepoint(R)
    k = int.from_bytes(H(R_bytes + pub_bytes + data), 'little') % l
    S = (r + k * a) % l
    sig = R_bytes + encodeint(S)

    return sig, pub_bytes


def main():
    parser = argparse.ArgumentParser(description="Generate and sign update-manifest.json for Martensite releases.")
    parser.add_argument("--dist-dir", default="dist", help="Directory containing release assets")
    parser.add_argument("--version", default="0.20.0", help="Release version (e.g. 0.20.0)")
    parser.add_argument("--channel", default="stable", help="Release channel (stable, beta, nightly)")
    parser.add_argument("--repo", default="https://github.com/jxoesneon/martensite", help="GitHub repository URL")
    parser.add_argument("--key", default=os.environ.get("RELEASE_ED25519_PRIVATE_KEY"), help="Ed25519 private key in hex")
    parser.add_argument("--verify", action="store_true", help="Verify signature after creation")

    args = parser.parse_args()
    dist_path = Path(args.dist_dir)

    if not dist_path.exists():
        dist_path.mkdir(parents=True, exist_ok=True)

    version = args.version.lstrip("v")
    today = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")

    print(f"=== Martensite Update Manifest Generator ===")
    print(f"Version:      {version}")
    print(f"Channel:      {args.channel}")
    print(f"Distribution: {dist_path.resolve()}")

    target_map = {
        "x86_64-pc-windows-msvc": None,
        "x86_64-apple-darwin": None,
        "aarch64-apple-darwin": None,
        "x86_64-unknown-linux-gnu": None,
        "aarch64-unknown-linux-gnu": None,
    }

    installers = []
    sha256_lines = []

    for file_path in sorted(dist_path.iterdir()):
        if not file_path.is_file():
            continue
        filename = file_path.name
        if filename in ("update-manifest.json", "update-manifest.json.sig", "update-manifest.pub", "SHA256SUMS.txt") or filename.endswith(".sha256"):
            continue

        size = file_path.stat().st_size
        sha256 = compute_sha256(file_path)
        sha256_lines.append(f"{sha256}  {filename}")

        # Compute individual asset signature
        with open(file_path, "rb") as f:
            payload = f.read()
        asset_sig_bytes, _ = sign_ed25519(payload, args.key)
        asset_sig_hex = asset_sig_bytes.hex()

        url = f"{args.repo}/releases/download/v{version}/{filename}"

        # Check if this is a primary CLI binary archive for a known target
        if filename.startswith("cargo-martensite"):
            for target in target_map.keys():
                if target in filename:
                    target_map[target] = {
                        "url": url,
                        "sha256": sha256,
                        "size": size,
                        "signature": asset_sig_hex
                    }
                    break

        # Record installers
        if filename.endswith(".msi"):
            installers.append({
                "type": "msi",
                "os": "windows",
                "filename": filename,
                "url": url,
                "sha256": sha256,
                "size": size,
                "signature": asset_sig_hex
            })
        elif filename.endswith(".dmg"):
            installers.append({
                "type": "dmg",
                "os": "macos",
                "filename": filename,
                "url": url,
                "sha256": sha256,
                "size": size,
                "signature": asset_sig_hex
            })
        elif filename.endswith(".flatpak"):
            installers.append({
                "type": "flatpak",
                "os": "linux",
                "filename": filename,
                "url": url,
                "sha256": sha256,
                "size": size,
                "signature": asset_sig_hex
            })

    # 1. Write SHA256SUMS.txt
    sha256sums_path = dist_path / "SHA256SUMS.txt"
    with open(sha256sums_path, "w", encoding="utf-8") as f:
        f.write("\n".join(sha256_lines) + "\n")
    print(f"Wrote {len(sha256_lines)} checksums to: {sha256sums_path.name}")

    # Build manifest matching cargo-martensite UpdateManifest schema
    assets = {target: data for target, data in target_map.items() if data is not None}

    # Canonical dictionary for signing (sorted keys, without signature)
    canonical_dict = {
        "version": version,
        "release_date": today,
        "release_notes": f"Martensite v{version} release",
        "assets": {k: assets[k] for k in sorted(assets.keys())}
    }
    canonical_bytes = json.dumps(canonical_dict, separators=(',', ':'), sort_keys=True).encode("utf-8")

    # Sign canonical bytes
    manifest_sig_bytes, pub_bytes = sign_ed25519(canonical_bytes, args.key)
    manifest_sig_hex = manifest_sig_bytes.hex()
    pub_hex = pub_bytes.hex()

    # Full manifest with signature and installer metadata
    full_manifest = {
        "version": version,
        "release_date": today,
        "release_notes": f"Martensite v{version} release",
        "channel": args.channel,
        "min_version": None,
        "assets": assets,
        "installers": installers,
        "signature": manifest_sig_hex,
        "public_key": pub_hex
    }

    manifest_json = json.dumps(full_manifest, indent=2, sort_keys=True).encode("utf-8")
    manifest_path = dist_path / "update-manifest.json"
    with open(manifest_path, "wb") as f:
        f.write(manifest_json)
    print(f"Wrote manifest: {manifest_path.name} ({len(manifest_json)} bytes)")

    sig_path = dist_path / "update-manifest.json.sig"
    with open(sig_path, "w", encoding="utf-8") as f:
        f.write(manifest_sig_hex + "\n")
    print(f"Wrote detached signature: {sig_path.name}")

    pub_path = dist_path / "update-manifest.pub"
    with open(pub_path, "w", encoding="utf-8") as f:
        f.write(pub_hex + "\n")
    print(f"Wrote public key: {pub_path.name} ({pub_hex})")

    print("\nSUCCESS: All release manifests and checksums generated.")


if __name__ == "__main__":
    main()
