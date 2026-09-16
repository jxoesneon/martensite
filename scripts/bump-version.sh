#!/usr/bin/env bash
# bump-version.sh [NEW_VERSION] — sync every version-bearing surface to each
# crate's own [package].version.
#
#   bump-version.sh          sync all surfaces to the current workspace version
#                            (drift repair)
#   bump-version.sh 0.18.0   set [workspace.package].version to 0.18.0, then
#                            sync all surfaces (release bump)
#
# Handles pinned vendored forks correctly: a dep's recorded version is synced
# to the target crate's [package].version, so martensite-vello /
# martensite-cosmic-text keep their independent pins.
# Surfaces: docs/VERSION_UPDATE_SURFACE.md. Verify: check-version-consistency.sh.

set -euo pipefail
cd "$(dirname "$0")/.."

NEW=${1:-}

WS_VER=$(awk '
    /^\[workspace\.package\]/ { inpkg=1; next }
    /^\[/                     { inpkg=0 }
    inpkg && /^version[ \t]*=/ { gsub(/"/, "", $3); print $3; exit }
' Cargo.toml)

# pkg_version — effective [package].version (literal, or WS_VER when the
# crate inherits via `version.workspace = true`).
pkg_version() {
    awk -v ws="$WS_VER" '
        /^\[package\]/ { inpkg=1; next }
        /^\[/          { inpkg=0 }
        inpkg && /^version\.workspace[ \t]*=[ \t]*true/ { print ws; exit }
        inpkg && /^version[ \t]*=/ {
            gsub(/"/, "", $3); print $3; exit
        }' "$1"
}

# set_package_version <manifest> <version> — rewrite [package].version.
set_package_version() {
    awk -v v="$2" '
        /^\[package\]/ { inpkg=1 }
        /^\[/ && !/^\[package\]/ { inpkg=0 }
        inpkg && /^version[ \t]*=/ && !done {
            sub(/"[^"]*"/, "\"" v "\""); done=1
        }
        { print }
    ' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
}

# ---------------------------------------------------------------------------
# 0. Optional workspace bump.
# ---------------------------------------------------------------------------
if [ -n "$NEW" ]; then
    awk -v v="$NEW" '
        /^\[workspace\.package\]/ { inpkg=1 }
        /^\[/ && !/^\[workspace\.package\]/ { inpkg=0 }
        inpkg && /^version[ \t]*=/ && !done {
            sub(/"[^"]*"/, "\"" v "\""); done=1
        }
        { print }
    ' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
    echo "workspace version -> $NEW"
    # Re-read after the bump.
    WS_VER=$(awk '
        /^\[workspace\.package\]/ { inpkg=1; next }
        /^\[/                     { inpkg=0 }
        inpkg && /^version[ \t]*=/ { gsub(/"/, "", $3); print $3; exit }
    ' Cargo.toml)
fi

# ---------------------------------------------------------------------------
# 1. Rewrite every `name = { ... path = "P" ... version = "V" ... }` dep line
#    (any order of keys) so V equals P's effective [package].version.
# ---------------------------------------------------------------------------
sync_dep_lines() { # <file> <path-prefix-for-relative-paths>
    local file=$1 dir=$2
    awk -v base="$dir" -v ws="$WS_VER" '
        function pkgver(p,   line, v) {
            v = ""
            while ((getline line < (p "/Cargo.toml")) > 0) {
                if (line ~ /^\[package\]/) inp=1
                else if (line ~ /^\[/) inp=0
                else if (inp && line ~ /^version\.workspace[ \t]*=[ \t]*true/) {
                    v = ws; break
                }
                else if (inp && line ~ /^version[ \t]*=/) {
                    sub(/.*"/, "", line); sub(/".*/, "", line); v = line; break
                }
            }
            close(p "/Cargo.toml")
            return v
        }
        /^[A-Za-z0-9_-]+ *= *\{/ && /path *= *"/ && /version *= *"/ {
            line = $0
            p = line; sub(/.*path *= *"/, "", p); sub(/".*/, "", p)
            full = (p ~ /^\//) ? p : base "/" p
            v = pkgver(full)
            if (v != "") {
                sub(/version *= *"[^"]*"/, "version = \"" v "\"", line)
                $0 = line
            }
        }
        { print }
    ' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
}

sync_dep_lines Cargo.toml .
for manifest in crates/martensite-bevy/Cargo.toml \
                crates/martensite-godot/Cargo.toml \
                examples/viewport_showcase/Cargo.toml; do
    set_package_version "$manifest" "$WS_VER"
    sync_dep_lines "$manifest" "$(dirname "$manifest")"
done
echo "manifests synced"

# ---------------------------------------------------------------------------
# 2. README/docs install snippets: `martensite-foo = "V"` and
#    `{ package = "martensite-foo", version = "V" }` -> crate's real version.
# ---------------------------------------------------------------------------
for f in $(grep -rlE '(martensite[a-z0-9_-]*|cargo-martensite) *= *["{]' \
            README.md docs crates/*/README.md crates/*/src/lib.rs 2>/dev/null); do
    awk -v ws="$WS_VER" '
        function pkgver(dir,   line, v) {
            v = ""
            while ((getline line < (dir "/Cargo.toml")) > 0) {
                if (line ~ /^\[package\]/) inp=1
                else if (line ~ /^\[/) inp=0
                else if (inp && line ~ /^version\.workspace[ \t]*=[ \t]*true/) {
                    v = ws; break
                }
                else if (inp && line ~ /^version[ \t]*=/) {
                    sub(/.*"/, "", line); sub(/".*/, "", line); v = line; break
                }
            }
            close(dir "/Cargo.toml")
            return v
        }
        function dirfor(name) {
            if (name == "martensite") return "crates/martensite"
            if (name == "cargo-martensite") return "tools/cargo-martensite"
            if (name ~ /^martensite-/) return "crates/" name
            return ""
        }
        {
            line = $0
            # inline form: dep = { package = "pkg", version = "V" } — matches any
            # dep name when the package resolves to a first-party crate.
            if (match(line, /\{[^}]*package *= *"martensite[a-z0-9_-]*"[^}]*version *= *"[^"]*"[^}]*\}/) ||
                match(line, /(martensite[a-z0-9_-]*|cargo-martensite) *= *\{[^}]*version *= *"[^"]*"/)) {
                seg = substr(line, RSTART, RLENGTH)
                name = seg; sub(/ *=.*/, "", name)
                pkg = seg
                if (pkg ~ /package *= *"/) { sub(/.*package *= *"/, "", pkg); sub(/".*/, "", pkg); name = pkg }
                d = dirfor(name)
                if (d != "") {
                    v = pkgver(d)
                    if (v != "") {
                        newseg = seg; sub(/version *= *"[^"]*"/, "version = \"" v "\"", newseg)
                        line = substr(line, 1, RSTART-1) newseg substr(line, RSTART+RLENGTH)
                    }
                }
            }
            # bare form: name = "V" — anywhere on the line, including
            # inline-code mentions like `martensite = "0.17"`.
            else if (match(line, /(martensite[a-z0-9_-]*|cargo-martensite) *= *"[^"]*"/)) {
                seg = substr(line, RSTART, RLENGTH)
                name = seg; sub(/ *=.*/, "", name)
                d = dirfor(name)
                if (d != "") {
                    v = pkgver(d)
                    if (v != "") {
                        newseg = seg; sub(/"[^"]*"$/, "\"" v "\"", newseg)
                        line = substr(line, 1, RSTART-1) newseg substr(line, RSTART+RLENGTH)
                    }
                }
            }
            print line
        }
    ' "$f" > "$f.tmp" && mv "$f.tmp" "$f"
done
echo "README/docs snippets synced"

# ---------------------------------------------------------------------------
# 3. Regenerate Cargo.lock.
# ---------------------------------------------------------------------------
cargo metadata --format-version 1 --quiet > /dev/null
echo "Cargo.lock regenerated"

echo
echo "Done. Run scripts/check-version-consistency.sh to verify."
