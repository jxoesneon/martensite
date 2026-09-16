#!/usr/bin/env bash
# check-version-consistency.sh — verify every version-bearing surface in the
# repo agrees with the canonical source: each martensite crate's own
# `[package].version` (and the `[workspace.package].version` for
# workspace-tracked crates).
#
# Surface inventory lives in docs/VERSION_UPDATE_SURFACE.md.
# Exit 0 = consistent; exit 1 = drift detected (details on stdout).

set -euo pipefail
cd "$(dirname "$0")/.."

RED=$'\033[31m'; GRN=$'\033[32m'; RST=$'\033[0m'
fail=0
err() { printf '%sFAIL%s %s\n' "$RED" "$RST" "$1"; fail=1; }
ok()  { printf '%s ok %s %s\n' "$GRN" "$RST" "$1"; }

WS_VER=$(awk '
    /^\[workspace\.package\]/ { inpkg=1; next }
    /^\[/                     { inpkg=0 }
    inpkg && /^version[ \t]*=/ { gsub(/"/, "", $3); print $3; exit }
' Cargo.toml)
[ -n "$WS_VER" ] || { echo "cannot read [workspace.package].version"; exit 2; }
echo "workspace version: $WS_VER"

# pkg_version <manifest-path> — the crate's effective [package].version:
# literal version, or the workspace version when `version.workspace = true`.
pkg_version() {
    awk -v ws="$WS_VER" '
        /^\[package\]/ { inpkg=1; next }
        /^\[/          { inpkg=0 }
        inpkg && /^version\.workspace[ \t]*=[ \t]*true/ { print ws; exit }
        inpkg && /^version[ \t]*=/ {
            gsub(/"/, "", $3); print $3; exit
        }' "$1"
}

# ---------------------------------------------------------------------------
# 1. [workspace.dependencies] path-dep versions must equal the target crate's
#    [package].version (covers pinned vendored forks automatically).
# ---------------------------------------------------------------------------
while IFS= read -r line; do
    dep=$(printf '%s' "$line" | sed -E 's/^([A-Za-z0-9_-]+).*/\1/')
    path=$(printf '%s' "$line" | sed -nE 's/.*path *= *"([^"]+)".*/\1/p')
    ver=$(printf '%s' "$line"  | sed -nE 's/.*version *= *"([^"]+)".*/\1/p')
    [ -n "$path" ] && [ -n "$ver" ] || continue
    actual=$(pkg_version "$path/Cargo.toml")
    if [ "$ver" != "$actual" ]; then
        err "Cargo.toml workspace dep '$dep' = \"$ver\" but $path is \"$actual\""
    fi
done < <(grep -nE '^[A-Za-z0-9_-]+ *= *\{[^}]*path *=' Cargo.toml | \
         grep -E 'martensite|accesskit_winit|vello|cosmic-text|cargo-martensite' | cut -d: -f2-)
ok "workspace.dependencies path-dep versions"

# ---------------------------------------------------------------------------
# 2. Excluded-but-workspace-tracked manifests: [package].version == WS_VER and
#    their martensite path-dep versions match the dep's own [package].version.
# ---------------------------------------------------------------------------
for manifest in crates/martensite-bevy/Cargo.toml \
                crates/martensite-godot/Cargo.toml \
                examples/viewport_showcase/Cargo.toml; do
    v=$(pkg_version "$manifest")
    [ "$v" = "$WS_VER" ] || err "$manifest [package].version = \"$v\" (expected $WS_VER)"
    while IFS= read -r line; do
        path=$(printf '%s' "$line" | sed -nE 's/.*path *= *"([^"]+)".*/\1/p')
        ver=$(printf '%s' "$line"  | sed -nE 's/.*version *= *"([^"]+)".*/\1/p')
        dep=$(printf '%s' "$line"  | sed -E 's/^([A-Za-z0-9_-]+).*/\1/')
        [ -n "$path" ] && [ -n "$ver" ] || continue
        actual=$(pkg_version "$(dirname "$manifest")/$path/Cargo.toml")
        [ "$ver" = "$actual" ] || \
            err "$manifest dep '$dep' = \"$ver\" but $path is \"$actual\""
    done < <(grep -nE '^martensite[A-Za-z0-9_-]* *= *\{[^}]*path *=' "$manifest" | cut -d: -f2-)
done
ok "excluded manifests (bevy/godot/viewport_showcase)"

# ---------------------------------------------------------------------------
# 3. Install snippets in README/docs must name the crate's real version.
#    Matches `martensite-foo = "V"` and inline `{ ..., version = "V" }` where
#    the package name resolves to a first-party crate.
# ---------------------------------------------------------------------------
check_snippet() { # <file> <crate-dir> <found-version>
    local file=$1 dir=$2 ver=$3 actual
    [ -f "$dir/Cargo.toml" ] || return 0
    actual=$(pkg_version "$dir/Cargo.toml")
    [ "$ver" = "$actual" ] || \
        err "$file: snippet version \"$ver\" for $(basename "$dir") (actual $actual)"
}
snippet_files() { grep -rlE 'martensite[a-z0-9_-]* *= *["{]' README.md docs crates/*/README.md 2>/dev/null || true; }
while IFS= read -r f; do
    # Form A: `name = "V"` bare, or `martensite-name = { ..., version = "V" }`.
    while IFS= read -r m; do
        name=$(printf '%s' "$m" | sed -E 's/^([A-Za-z0-9_-]+).*/\1/')
        ver=$(printf '%s' "$m"  | sed -nE 's/.*version *= *"([^"]+)".*/\1/p')
        [ -n "$ver" ] || ver=$(printf '%s' "$m" | sed -nE 's/.*= *"([0-9][^"]*)".*/\1/p')
        pkg=$(printf '%s' "$m" | sed -nE 's/.*package *= *"([^"]+)".*/\1/p')
        [ -n "$pkg" ] && name=$pkg
        [ -n "$ver" ] || continue
        case "$name" in
            martensite)            check_snippet "$f" crates/martensite "$ver" ;;
            martensite-*)          check_snippet "$f" "crates/$name" "$ver" ;;
            cargo-martensite)      check_snippet "$f" tools/cargo-martensite "$ver" ;;
        esac
    done < <(grep -oE '(martensite[a-z0-9_-]*|cargo-martensite) *= *(("[0-9][^"]*")|(\{[^}]*\}))' "$f")
    # Form B: aliased `{ package = "martensite-foo", version = "V" }`.
    while IFS= read -r m; do
        name=$(printf '%s' "$m" | sed -nE 's/.*package *= *"([^"]+)".*/\1/p')
        ver=$(printf '%s' "$m"  | sed -nE 's/.*version *= *"([^"]+)".*/\1/p')
        [ -n "$ver" ] && [ -n "$name" ] || continue
        check_snippet "$f" "crates/$name" "$ver"
    done < <(grep -oE '\{[^}]*package *= *"martensite[a-z0-9_-]*"[^}]*version *= *"[0-9][^"]*"[^}]*\}' "$f")
done < <(snippet_files)
ok "README/docs install snippets"

# ---------------------------------------------------------------------------
# 4. CHANGELOG has a section for the current workspace version.
# ---------------------------------------------------------------------------
if grep -qE "^## \[v?${WS_VER}\]" CHANGELOG.md; then
    ok "CHANGELOG section for $WS_VER"
else
    err "CHANGELOG.md has no '## [$WS_VER]' section"
fi

# ---------------------------------------------------------------------------
# 5. Cargo.lock agrees with manifests (catches missed regeneration).
# ---------------------------------------------------------------------------
if [ -f Cargo.lock ]; then
    # name -> expected-version map (tab-separated) from each first-party
    # manifest; then compare each [[package]] lock entry against its own
    # crate's version. Portable (no bash-4 assoc arrays).
    mapfile=$(mktemp)
    for m in crates/*/Cargo.toml tools/cargo-martensite/Cargo.toml; do
        name=$(awk '/^name[ \t]*=/{gsub(/"/,"",$3); print $3; exit}' "$m")
        [ -n "$name" ] && printf '%s\t%s\n' "$name" "$(pkg_version "$m")" >> "$mapfile"
    done
    stale=$(awk '
        /^\[\[package\]\]/ { if (name != "") print name" "ver; name=""; ver="" }
        /^name = /         { gsub(/"/,"",$3); name=$3 }
        /^version = /      { gsub(/"/,"",$3); ver=$3 }
        END                { if (name != "") print name" "ver }
    ' Cargo.lock | while IFS=' ' read -r name ver; do
        exp=$(awk -v n="$name" -F'\t' '$1==n{print $2}' "$mapfile")
        if [ -n "$exp" ] && [ "$ver" != "$exp" ]; then
            printf '%s %s (expected %s)\n' "$name" "$ver" "$exp"
        fi
    done)
    rm -f "$mapfile"
    if [ -n "$stale" ]; then
        err "Cargo.lock out of sync:\n$stale"
    else
        ok "Cargo.lock in sync"
    fi
fi

echo
if [ "$fail" -eq 0 ]; then
    printf '%sAll version surfaces consistent.%s\n' "$GRN" "$RST"
else
    printf '%sVersion drift detected — run scripts/bump-version.sh to sync.%s\n' "$RED" "$RST"
    exit 1
fi
