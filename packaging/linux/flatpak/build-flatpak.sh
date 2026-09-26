#!/usr/bin/env bash
# ==============================================================================
# build-flatpak.sh — Build Flatpak package and single-file bundle for Widget Catalog
# ==============================================================================
# Uses flatpak-builder to build org.martensite.WidgetCatalog.yaml, export to an OSTree
# repo, and create a standalone .flatpak single-file bundle.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

MANIFEST="${SCRIPT_DIR}/org.martensite.WidgetCatalog.yaml"
APP_ID="org.martensite.WidgetCatalog"
OUTPUT_DIR="${REPO_ROOT}/dist"
BUILD_DIR="${REPO_ROOT}/target/flatpak-build"
REPO_DIR="${REPO_ROOT}/target/flatpak-repo"
BUNDLE_ONLY=0

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
    -o, --output-dir <DIR>   Output directory for .flatpak bundle (default: dist/).
        --bundle-only        Create single-file .flatpak bundle only (assumes repo exists).
    -h, --help               Show this help message.
EOF
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -o|--output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --bundle-only)
            BUNDLE_ONLY=1
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

echo "=== Martensite Linux Flatpak Packaging ==="

# 1. Check prerequisites
if ! command -v flatpak >/dev/null 2>&1; then
    echo "Error: 'flatpak' is not installed or not in PATH." >&2
    echo "Install via your Linux package manager (e.g. sudo apt install flatpak / dnf install flatpak)." >&2
    exit 1
fi

if ! command -v flatpak-builder >/dev/null 2>&1; then
    echo "Error: 'flatpak-builder' is not installed or not in PATH." >&2
    echo "Install via your Linux package manager (e.g. sudo apt install flatpak-builder / dnf install flatpak-builder)." >&2
    exit 1
fi

# 2. Extract Version
VERSION=""
if [[ -f "${REPO_ROOT}/Cargo.toml" ]]; then
    VERSION="$(grep -m1 '^version' "${REPO_ROOT}/Cargo.toml" | sed -E 's/version *= *"([^"]+)".*/\1/')"
fi
VERSION="${VERSION:-0.20.0}"
CLEAN_VERSION="${VERSION#v}"
ARCH="$(flatpak --default-arch 2>/dev/null || uname -m)"

echo "App ID:       ${APP_ID}"
echo "Version:      ${CLEAN_VERSION}"
echo "Architecture: ${ARCH}"
echo "Manifest:     ${MANIFEST}"

mkdir -p "${OUTPUT_DIR}"
mkdir -p "${REPO_DIR}"

# 3. Build using flatpak-builder
if [[ "${BUNDLE_ONLY}" -eq 0 ]]; then
    echo "Running flatpak-builder..."
    flatpak-builder \
        --force-clean \
        --repo="${REPO_DIR}" \
        --install-deps-from=flathub \
        --default-branch=stable \
        "${BUILD_DIR}" \
        "${MANIFEST}"
fi

# 4. Export single-file .flatpak bundle
BUNDLE_NAME="Martensite-WidgetCatalog-${CLEAN_VERSION}-${ARCH}.flatpak"
BUNDLE_PATH="${OUTPUT_DIR}/${BUNDLE_NAME}"

echo "Exporting single-file bundle: ${BUNDLE_PATH}..."
flatpak build-bundle \
    --arch="${ARCH}" \
    "${REPO_DIR}" \
    "${BUNDLE_PATH}" \
    "${APP_ID}" \
    stable

# 5. Compute SHA-256 checksum
echo "Generating SHA-256 checksum..."
cd "${OUTPUT_DIR}"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${BUNDLE_NAME}" > "${BUNDLE_NAME}.sha256"
fi

echo "SUCCESS: Flatpak bundle generated successfully!"
echo "File:   ${BUNDLE_PATH}"
echo "Size:   $(stat -c%s "${BUNDLE_PATH}" 2>/dev/null || wc -c < "${BUNDLE_PATH}") bytes"
EOF
