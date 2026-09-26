#!/usr/bin/env bash
# ==============================================================================
# build-dmg.sh — Build macOS .dmg package for Martensite Widget Catalog
# ==============================================================================
# Assembles WidgetCatalog.app bundle, configures DMG icon/layout, adds /Applications
# symlink, optionally signs/notarizes, and emits compressed UDZO .dmg.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VERSION=""
ARCH=""
BIN_DIR=""
OUTPUT_DIR="${REPO_ROOT}/dist"
SKIP_BUILD=0

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
    -v, --version <VER>      Version string (e.g. 0.20.0). Defaults to Cargo.toml.
    -a, --arch <ARCH>        Architecture (x86_64, aarch64, universal2). Defaults to host.
    -b, --bin-dir <DIR>      Directory containing built widget_catalog binary.
    -o, --output-dir <DIR>   Output directory for .dmg (default: dist/).
        --skip-build         Do not invoke cargo build if binary is missing.
    -h, --help               Show this help message.
EOF
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -v|--version)
            VERSION="$2"
            shift 2
            ;;
        -a|--arch)
            ARCH="$2"
            shift 2
            ;;
        -b|--bin-dir)
            BIN_DIR="$2"
            shift 2
            ;;
        -o|--output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage
            ;;
    esac
done

echo "=== Martensite macOS DMG Packaging ==="

# 1. Determine Version
if [[ -z "${VERSION}" ]]; then
    if [[ -f "${REPO_ROOT}/Cargo.toml" ]]; then
        VERSION="$(grep -m1 '^version' "${REPO_ROOT}/Cargo.toml" | sed -E 's/version *= *"([^"]+)".*/\1/')"
    fi
    VERSION="${VERSION:-0.20.0}"
fi
CLEAN_VERSION="${VERSION#v}"
echo "Target Version: ${CLEAN_VERSION}"

# 2. Determine Architecture
if [[ -z "${ARCH}" ]]; then
    ARCH="$(uname -m)"
fi
echo "Architecture: ${ARCH}"

# 3. Resolve Binaries
if [[ -z "${BIN_DIR}" ]]; then
    CANDIDATE_DIRS=(
        "${REPO_ROOT}/target/${ARCH}-apple-darwin/release"
        "${REPO_ROOT}/target/release"
    )
    for dir in "${CANDIDATE_DIRS[@]}"; do
        if [[ -f "${dir}/widget_catalog" ]]; then
            BIN_DIR="${dir}"
            break
        fi
    done
fi

if [[ -z "${BIN_DIR}" || ! -f "${BIN_DIR}/widget_catalog" ]]; then
    if [[ "${SKIP_BUILD}" -eq 1 ]]; then
        echo "Error: widget_catalog binary not found in '${BIN_DIR:-}' and --skip-build was specified." >&2
        exit 1
    fi
    echo "Building widget_catalog and cargo-martensite with Cargo..."
    (cd "${REPO_ROOT}" && cargo build --release --locked -p widget_catalog -p cargo-martensite)
    BIN_DIR="${REPO_ROOT}/target/release"
fi

WIDGET_CATALOG_BIN="${BIN_DIR}/widget_catalog"
CARGO_MARTENSITE_BIN="${BIN_DIR}/cargo-martensite"

if [[ ! -f "${WIDGET_CATALOG_BIN}" ]]; then
    echo "Error: Missing required binary: ${WIDGET_CATALOG_BIN}" >&2
    exit 1
fi

echo "Using binaries from: ${BIN_DIR}"

# 4. Prepare Staging and Bundle Structure
BUILD_DIR="${REPO_ROOT}/target/dmg-build"
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}"
mkdir -p "${OUTPUT_DIR}"

APP_BUNDLE="${BUILD_DIR}/Martensite Widget Catalog.app"
CONTENTS="${APP_BUNDLE}/Contents"
MACOS_DIR="${CONTENTS}/MacOS"
RESOURCES_DIR="${CONTENTS}/Resources"

mkdir -p "${MACOS_DIR}"
mkdir -p "${RESOURCES_DIR}"

# Copy binary
cp "${WIDGET_CATALOG_BIN}" "${MACOS_DIR}/widget_catalog"
chmod +x "${MACOS_DIR}/widget_catalog"

# Include CLI tool in helper directory if available
if [[ -f "${CARGO_MARTENSITE_BIN}" ]]; then
    cp "${CARGO_MARTENSITE_BIN}" "${MACOS_DIR}/cargo-martensite"
    chmod +x "${MACOS_DIR}/cargo-martensite"
fi

# Generate Info.plist
cat > "${CONTENTS}/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>widget_catalog</string>
    <key>CFBundleIdentifier</key>
    <string>org.martensite.WidgetCatalog</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Widget Catalog</string>
    <key>CFBundleDisplayName</key>
    <string>Martensite Widget Catalog</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${CLEAN_VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${CLEAN_VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright © 2026 Martensite Project. Apache-2.0 OR MIT.</string>
</dict>
</plist>
EOF

# Copy Icon if present
if [[ -f "${SCRIPT_DIR}/AppIcon.icns" ]]; then
    cp "${SCRIPT_DIR}/AppIcon.icns" "${RESOURCES_DIR}/AppIcon.icns"
fi

# 5. Code Signing
SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-${DEVELOPER_ID_APPLICATION:-}}"
if [[ -n "${SIGNING_IDENTITY}" ]]; then
    echo "Signing application bundle with identity: ${SIGNING_IDENTITY}..."
    codesign --force --options runtime --deep --sign "${SIGNING_IDENTITY}" "${APP_BUNDLE}"
else
    echo "Notice: No Apple signing identity configured (APPLE_SIGNING_IDENTITY / DEVELOPER_ID_APPLICATION)."
    echo "        Application bundle will remain unsigned (ad-hoc development fallback)."
fi

# 6. Stage DMG folder
STAGING_DIR="${BUILD_DIR}/dmg-root"
mkdir -p "${STAGING_DIR}"
cp -R "${APP_BUNDLE}" "${STAGING_DIR}/"

# Add /Applications symlink for drag-and-drop installation
ln -s /Applications "${STAGING_DIR}/Applications"

# Copy license if available
if [[ -f "${REPO_ROOT}/LICENSE-MIT" ]]; then
    cp "${REPO_ROOT}/LICENSE-MIT" "${STAGING_DIR}/LICENSE.txt"
fi

# 7. Create DMG
DMG_NAME="Martensite-WidgetCatalog-${CLEAN_VERSION}-${ARCH}.dmg"
DMG_PATH="${OUTPUT_DIR}/${DMG_NAME}"
rm -f "${DMG_PATH}"

echo "Creating DMG at: ${DMG_PATH}..."

if command -v create-dmg >/dev/null 2>&1; then
    echo "Using create-dmg for styled installer presentation..."
    create-dmg \
        --volname "Martensite Widget Catalog" \
        --window-pos 200 120 \
        --window-size 660 400 \
        --icon-size 128 \
        --icon "Martensite Widget Catalog.app" 180 190 \
        --app-drop-link 480 190 \
        --hide-extension "Martensite Widget Catalog.app" \
        --no-internet-enable \
        "${DMG_PATH}" \
        "${STAGING_DIR}" || {
            echo "create-dmg exited with non-zero; falling back to hdiutil..."
            hdiutil create -volname "Martensite Widget Catalog" \
                -srcfolder "${STAGING_DIR}" \
                -ov -format UDZO \
                "${DMG_PATH}"
        }
else
    echo "create-dmg not found; using native macOS hdiutil..."
    hdiutil create -volname "Martensite Widget Catalog" \
        -srcfolder "${STAGING_DIR}" \
        -ov -format UDZO \
        "${DMG_PATH}"
fi

# 8. Notarization (if credentials present)
if [[ -n "${APPLE_ID:-}" && -n "${APPLE_APP_SPECIFIC_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
    echo "Submitting DMG for Apple notarization..."
    xcrun notarytool submit "${DMG_PATH}" \
        --apple-id "${APPLE_ID}" \
        --password "${APPLE_APP_SPECIFIC_PASSWORD}" \
        --team-id "${APPLE_TEAM_ID}" \
        --wait
    echo "Stapling notarization ticket to DMG..."
    xcrun stapler staple "${DMG_PATH}"
else
    echo "Notice: Notarization credentials not configured; skipping notarytool submission."
fi

# 9. Compute Checksum
echo "Generating SHA-256 checksum..."
cd "${OUTPUT_DIR}"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${DMG_NAME}" > "${DMG_NAME}.sha256"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${DMG_NAME}" > "${DMG_NAME}.sha256"
fi

echo "SUCCESS: macOS DMG created successfully!"
echo "File:   ${DMG_PATH}"
echo "Size:   $(stat -f%z "${DMG_PATH}" 2>/dev/null || wc -c < "${DMG_PATH}") bytes"
EOF
