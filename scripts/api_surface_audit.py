#!/usr/bin/env python3
"""Enumerate the public API surface of every publishable Martensite crate and
regenerate the machine-managed section of docs/API_SURFACE_AUDIT.md.

How it works
------------
For every workspace member whose Cargo.toml does not set ``publish = false``
the script runs

    RUSTC_BOOTSTRAP=1 cargo doc -p <crate> --no-deps \
        -Z unstable-options --output-format json

(``RUSTC_BOOTSTRAP`` lets stable rustdoc emit its unstable JSON format; the
script only consumes it, it never ships in any build path.)

Each crate is documented twice:

1. a *default-features* pass, and
2. a *feature* pass enabling the crate's optional public features
   (``EXTRA_FEATURES`` below), so feature-gated API items are counted and
   tagged with the feature that exposes them.

Items that only appear in the feature pass are marked with the enabling
feature set; EXPERIMENTAL classification comes from ``EXPERIMENTAL_RULES``
(path regexes per crate) and ``CRATE_TIER`` (crate-level classification for
vendored forks). Re-export aliases are resolved through the rustdoc
``paths``/``index`` tables to their canonical target item and *inherit*
that item's tier — otherwise module-scoped rules (``::docking(::|$)``)
never match root aliases like ``martensite_blessed::DockArea``, whose
paths drop the module segment. The merged result replaces the text between

    <!-- BEGIN GENERATED: api-surface --> / <!-- END GENERATED -->

in docs/API_SURFACE_AUDIT.md. Run from anywhere:

    python3 scripts/api_surface_audit.py [--skip-docgen] [--strict]

``--skip-docgen`` reuses the rustdoc JSON already in ``target/doc/*.json``
(after a manual doc pass) instead of invoking cargo. ``--strict`` exits
nonzero when any crate fails doc generation instead of only noting the
failure in the output. Requires python3 only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC_OUT = ROOT / "docs" / "API_SURFACE_AUDIT.md"
TARGET_DOC = ROOT / "target" / "doc"

BEGIN = "<!-- BEGIN GENERATED: api-surface -->"
END = "<!-- END GENERATED: api-surface -->"

# ---------------------------------------------------------------------------
# Per-crate extra feature sets for the second doc pass. Keep this list in sync
# with each crate's [features] table; platform-only features whose system
# dependencies cannot be installed on a documentation host (decoder-ffmpeg,
# decoder-vaapi, hot_reload, windows/wayland shell backends) are intentionally
# absent — those items are feature-gated leaves inside modules that are
# already classified EXPERIMENTAL.
# ---------------------------------------------------------------------------
EXTRA_FEATURES: dict[str, str] = {
    # NOTE: the `decoder` umbrella feature pulls decoder-ffmpeg + decoder-vaapi
    # (ffmpeg-next, cros-libva), which need system libraries a doc host may not
    # have. Enumerate the portable backends instead; the umbrella adds no API
    # items of its own.
    "martensite": "native-fallback,test-noop,decoder-videotoolbox,decoder-mf",
    "martensite-accesskit-winit": "accesskit_android",
    "martensite-assets": "reactive",
    "martensite-blessed": "serde",
    "martensite-clipboard": "platform,wayland",
    "martensite-core": "devtools-timemachine",
    "martensite-cosmic-text": "--all-features",
    "martensite-devtools": "render,devtools-timemachine",
    "martensite-engine-bridge": "test-noop",
    "martensite-media": "decoder-videotoolbox,decoder-mf,test-noop",
    "martensite-media-platform": "decoder-videotoolbox,decoder-mf",
    "martensite-reactive": "devtools-timemachine",
    "martensite-render": "vello",
    "martensite-shell": "macos-backend",
    "martensite-vello": "wgpu,wgpu_default,bump_estimate,debug_layers,wgpu-profiler",
    "martensite-wgpu": "vello,test-noop",
}

# ---------------------------------------------------------------------------
# Crate-level classification. Vendored forks track upstream APIs and are
# EXPERIMENTAL by definition — their surface changes when upstream re-syncs.
# ---------------------------------------------------------------------------
CRATE_TIER: dict[str, str] = {
    "martensite-cosmic-text": "VENDORED",
    "martensite-vello": "VENDORED",
    "martensite-accesskit-winit": "VENDORED",
    "martensite-access-platform": "EXPERIMENTAL",
    "martensite-engine-bridge": "EXPERIMENTAL",
    "martensite-host": "EXPERIMENTAL",
}

# Path-prefix rules marking EXPERIMENTAL items inside otherwise stable crates.
# Each entry is (regex matched against the canonical `a::b::C` path, reason).
EXPERIMENTAL_RULES: dict[str, list[tuple[str, str]]] = {
    "martensite-wgpu": [
        (r"::resilience(::|$)", "device-loss recovery API added in v0.11.0, still hardening"),
        (r"::theme_transition(::|$)", "zero-allocation theme transitions, perf-gated and young"),
        (r"::external(::|$)", "external-engine embedding bridge surface (v0.17.0)"),
        (r"::web(::|$)", "wasm/web backend glue, compile-verified only"),
    ],
    "martensite-media": [
        (r"::decoder(::|$)|decoder", "decoder pipeline added in v0.16.0, hardware-verified backends pending"),
    ],
    "martensite-media-platform": [
        (r"::decoder(::|$)|decoder", "decoder wire types/backends added in v0.16.0"),
        (r"import_", "hardware surface import FFI; wgpu-hal API still evolving"),
    ],
    "martensite-plugin": [
        (r"::runtime(::|$)", "plugin runtime ABI added in v0.11.0, ecosystem immature"),
        (r"::ring_buffer(::|$)", "plugin IPC ring buffer, part of the runtime ABI"),
        (r"::security(::|$)", "plugin sandbox/policy surface, ecosystem immature"),
    ],
    "martensite-blessed": [
        (r"::docking(::|$)", "docking workspace framework added in v0.15.0"),
        (r"::code_editor(::|$)", "complex widget, API still settling"),
        (r"::data_table(::|$)", "complex widget, API still settling"),
        (r"::chart(::|$)", "complex widget, API still settling"),
        (r"::audio_waveform(::|$)", "complex widget, API still settling"),
    ],
    "martensite-devtools": [
        (r"::timemachine(::|$)", "time-travel debugging behind devtools-timemachine (v0.17.0)"),
    ],
    "martensite-core": [
        (r"::snapshot(::|$)|timemachine|Timemachine|Arena(State|RestoreError)",
         "devtools-timemachine arena snapshot/restore surface"),
    ],
    "martensite-reactive": [
        (r"journal|Journal|SignalSnapshot|WriteRecord",
         "devtools-timemachine write journal + signal snapshots"),
    ],
    "martensite-assets": [
        (r"ReactiveVfsWatcher|::reactive(::|$)",
         "reactive VFS watcher behind the `reactive` feature"),
    ],
    "martensite-shell": [
        (r"::platform_impl(::|$)", "per-OS shell backends behind platform features"),
        (r"::status_notifier(::|$)", "StatusNotifierItem tray protocol, Linux-only"),
    ],
    "martensite": [
        (r"::devtools|::hot_reload|::external", "umbrella re-exports of experimental subsystems"),
        (r"decoder", "decoder pipeline added in v0.16.0"),
        (r"::widgets::external", "external-engine widget embedding (v0.17.0)"),
    ],
    "martensite-window": [
        (r"::web(::|$)", "wasm/web backend glue, compile-verified only"),
        (r"::stylus(::|$)", "Kalman stylus filtering, latency-gated and young"),
        (r"::csd(::|$)|csd_region", "client-side decoration hit-testing, young shell surface"),
    ],
}

# Item kinds (rustdoc JSON `kind`) reported in the audit, and how they are
# grouped in the per-crate digest. `variant` is folded into the enum count.
KIND_LABEL = {
    "struct": "structs",
    "enum": "enums",
    "trait": "traits",
    "function": "functions",
    "constant": "constants",
    "static": "statics",
    "type_alias": "type aliases",
    "module": "modules",
    "macro": "macros",
    "union": "unions",
    "proc_attribute": "proc-macro attributes",
    "proc_derive": "proc-macro derives",
    "proc_function": "proc-macro functions",
    "trait_alias": "trait aliases",
    "reexport": "re-exports",
    "reexport_glob": "glob re-exports",
}
LIST_LIMIT = 160  # above this, emit a per-module digest instead of full list


def cargo_metadata() -> list[dict]:
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    meta = json.loads(out.stdout)
    crates = []
    for pkg in meta["packages"]:
        manifest = Path(pkg["manifest_path"])
        if ROOT / "crates" not in manifest.parents:
            continue  # tools/, examples/, benches/, stubs are out of scope
        if pkg.get("publish") == []:  # publish = false
            continue
        lib = next((t["name"] for t in pkg["targets"]
                    if "lib" in t["kind"] or "proc-macro" in t["kind"]),
                   pkg["name"].replace("-", "_"))
        crates.append({"name": pkg["name"], "version": pkg["version"],
                       "lib": lib, "manifest": manifest})
    crates.sort(key=lambda c: c["name"])
    return crates


def rustdoc_json(crate: dict, features: str | None) -> Path | None:
    """Doc one crate, return the path to its rustdoc JSON (or None)."""
    cmd = ["cargo", "doc", "-p", crate["name"], "--no-deps",
           "-Z", "unstable-options", "--output-format", "json"]
    if features:
        if features == "--all-features":
            cmd.append("--all-features")
        else:
            cmd += ["--features", features]
    env = dict(__import__("os").environ, RUSTC_BOOTSTRAP="1")
    proc = subprocess.run(cmd, cwd=ROOT, env=env,
                          capture_output=True, text=True)
    json_path = TARGET_DOC / (crate["lib"] + ".json")
    if proc.returncode != 0 or not json_path.exists():
        tail = proc.stderr.strip().splitlines()
        sys.stderr.write(f"  ! doc failed for {crate['name']} "
                         f"({features or 'default'}): "
                         f"{tail[-1] if tail else 'no json emitted'}\n")
        return None
    return json_path


def qualify_source(source: str, module: str, crate: str,
                   externs: set[str]) -> tuple[str, str]:
    """Resolve a `use` item's ``source`` (as written) into a
    crate-qualified path plus the crate that path belongs to.

    Rust `use` paths are relative to the module containing the item unless
    they start with ``crate``, ``self``, ``super``, or an extern crate
    name. The qualified path preserves intermediate module segments that
    canonical resolution drops: ``pub use chart::Point`` at the root of
    ``martensite_blessed`` *transits* the experimental ``chart`` module
    even though the item itself resolves to ``kurbo::point::Point``.
    """
    head, _, rest = source.partition("::")
    if head == "crate":
        return (f"{crate}::{rest}" if rest else crate), crate
    if head == "self":
        return (f"{module}::{rest}" if rest else module), crate
    if head == "super":
        parent = module.rpartition("::")[0] or crate
        return (f"{parent}::{rest}" if rest else parent), crate
    if head in externs or head == crate:
        return source, head
    return f"{module}::{source}", crate


def resolve_use(doc: dict, u: dict) -> tuple[str | None, str | None]:
    """Follow a `use` item's target Id to its canonical path.

    Returns ``(canonical_path, defining_crate)`` — the defining crate named
    in rustdoc form (underscored, e.g. ``martensite_blessed``) — or
    ``(None, None)`` when the target cannot be resolved. ``use`` → ``use``
    chains are followed until a non-``use`` item is reached.
    """
    idx = doc["index"]
    tid = u.get("id")
    seen: set[str] = set()
    while tid is not None and str(tid) not in seen:
        seen.add(str(tid))
        target = idx.get(str(tid))
        if target is not None and "use" in target["inner"]:
            tid = target["inner"]["use"].get("id")
            continue
        summary = doc["paths"].get(str(tid))
        if summary is None:
            break
        cpath = "::".join(summary["path"])
        cid = summary["crate_id"]
        if cid == 0:
            ccrate = idx[str(doc["root"])]["name"]
        else:
            ccrate = doc.get("external_crates", {}).get(str(cid), {}).get("name")
        return cpath, ccrate
    return None, None


def load_items(json_path: Path) -> tuple[dict[str, str], dict[str, dict]]:
    """Return ({public_path: kind}, {reexport_path: meta}) for the crate's
    own public items.

    `paths` gives each defined item's canonical path. Re-exports (`pub use`)
    are separate index items and are collected by walking the public module
    tree, since they create additional nameable surface (this is how the
    `martensite` umbrella crate's API is actually reached). For each
    re-export, `meta` records:

    - ``src`` — the source path as written in the `use` item (relative,
      e.g. ``ring_buffer::PluginRuntime``, so not directly rule-matchable),
    - ``qsrc`` / ``qsrc_crate`` — the source path *qualified* into the
      using module's scope (``chart::Point`` at the crate root becomes
      ``martensite_blessed::chart::Point``), so transit through an
      experimental module is visible even when the item's canonical path
      leaves the crate,
    - ``target`` — the *resolved* canonical path of the re-exported item
      (e.g. ``martensite_plugin::ring_buffer::PluginRuntime``), and
    - ``target_crate`` — the crate defining that item.

    The resolved target lets an alias inherit the canonical item's
    classification: module-scoped EXPERIMENTAL_RULES (``::docking(::|$)``)
    match the canonical path but never the alias path, which drops the
    module segment.
    """
    doc = json.loads(json_path.read_text())
    idx = doc["index"]
    crate_name = idx[str(doc["root"])]["name"]
    items: dict[str, str] = {}
    for entry in doc["paths"].values():
        if entry["crate_id"] != 0:
            continue
        path = "::".join(entry["path"])
        if path == crate_name:
            continue  # the crate root itself
        kind = entry["kind"]
        if kind == "variant":
            continue  # counted with the parent enum
        if kind not in KIND_LABEL:
            continue
        items[path] = kind

    # Public re-exports: parent module path + the use item's name.
    mod_path = {iid: "::".join(p["path"])
                for iid, p in doc["paths"].items()
                if p["crate_id"] == 0 and p["kind"] == "module"}
    reexport_meta: dict[str, dict] = {}
    externs = {v["name"] for v in doc.get("external_crates", {}).values()}
    for iid, item in idx.items():
        if "module" not in item["inner"] or iid not in mod_path:
            continue
        for child_id in item["inner"]["module"].get("items", []):
            child = idx.get(str(child_id))
            if child is None or "use" not in child["inner"]:
                continue
            if child["visibility"] != "public":
                continue
            u = child["inner"]["use"]
            cpath, ccrate = resolve_use(doc, u)
            qsrc, qcrate = qualify_source(u["source"], mod_path[iid],
                                          crate_name, externs)
            if u.get("is_glob"):
                path = f"{mod_path[iid]}::* (from {u['source']})"
                items.setdefault(path, "reexport_glob")
            else:
                path = f"{mod_path[iid]}::{u['name']}"
                items.setdefault(path, "reexport")
            reexport_meta.setdefault(
                path, {"src": u["source"], "qsrc": qsrc,
                       "qsrc_crate": qcrate, "target": cpath,
                       "target_crate": ccrate})
    return items, reexport_meta


def load_deprecated(json_path: Path) -> list[str]:
    doc = json.loads(json_path.read_text())
    paths = {iid: p["path"] for iid, p in doc["paths"].items()
             if p["crate_id"] == 0}
    out = []
    for iid, item in doc["index"].items():
        if item.get("deprecation") and iid in paths:
            out.append("::".join(paths[iid]))
    return sorted(out)


def _tier_of(crate: str, path: str) -> str | None:
    """Tier implied by `path` living in `crate` (underscored or hyphenated
    crate name), or None when no rule/tier matches."""
    key = crate.replace("_", "-")
    tier = CRATE_TIER.get(key)
    if tier:
        return tier
    for pattern, _why in EXPERIMENTAL_RULES.get(key, []):
        if re.search(pattern, path):
            return "EXPERIMENTAL"
    return None


def classify(crate: str, path: str, info: dict | None = None) -> str:
    """Classify one public path. `info` is the per-item dict built by
    `merge_into`/`load_items` (kind/feature plus, for re-exports, `src`,
    `qsrc`/`qsrc_crate`, `target`/`target_crate`)."""
    info = info or {}
    tier = _tier_of(crate, path)
    if tier:
        return tier
    src = info.get("src")
    for pattern, _why in EXPERIMENTAL_RULES.get(crate, []):
        if src and re.search(pattern, src):
            return "EXPERIMENTAL"
    # Transit check: the as-written source path may pass through an
    # experimental module even when the item's canonical path does not —
    # `pub use chart::Point` at the root of `martensite_blessed` reaches
    # `kurbo::point::Point`, but the alias is part of the experimental
    # `chart` surface.
    qsrc = info.get("qsrc")
    if qsrc:
        qtier = _tier_of(info.get("qsrc_crate") or crate, qsrc)
        if qtier:
            return qtier
    # Inheritance: a re-export alias takes the tier of the canonical item
    # it resolves to. Module-scoped rules are written against canonical
    # paths; the alias path (e.g. `martensite_blessed::Chart`) drops the
    # module segment (`chart`), so without this step every root alias of
    # an experimental module would be misclassified STABLE.
    target = info.get("target")
    if target:
        ttier = _tier_of(info.get("target_crate") or crate, target)
        if ttier:
            return ttier
    return "STABLE"


def module_of(path: str) -> str:
    parts = path.split("::")
    return parts[1] if len(parts) > 2 else "(crate root)"


def whole_crate_alias_caveats(items: dict[str, dict]) -> list[str]:
    """One caveat string per alias re-exporting an entire crate root whose
    crate is only *partially* experimental — the alias's own tag cannot
    reflect the experimental sub-surface reachable through it (the
    individual sub-paths are not enumerated in this crate's JSON)."""
    out = []
    for path, i in items.items():
        target, tcrate = i.get("target"), i.get("target_crate")
        if not target or "::" in target:
            continue  # not a crate-root alias
        rules = EXPERIMENTAL_RULES.get((tcrate or "").replace("_", "-"))
        if not rules:
            continue
        whys = "; ".join(sorted({w for _p, w in rules}))
        out.append(f"`{path}` re-exports the whole of `{target}`, which "
                   f"contains experimental sub-surface ({whys}) not "
                   "reflected in the alias's stable tag.")
    return sorted(out)


def emit_crate(crate: str, version: str, items: dict[str, dict],
               deprecated: list[str]) -> str:
    """items: {path: {'kind': kind, 'feature': str|None, 'src': str|None,
    'qsrc': str|None, 'qsrc_crate': str|None, 'target': str|None,
    'target_crate': str|None}}"""
    total = len(items)
    cls = {p: classify(crate, p, i) for p, i in items.items()}
    stable = [p for p in items if cls[p] == "STABLE"]
    experimental = [p for p in items if cls[p] == "EXPERIMENTAL"]
    vendored = [p for p in items if cls[p] == "VENDORED"]

    counts = Counter(KIND_LABEL[i["kind"]] for i in items.values())
    count_s = ", ".join(f"{v} {k}" for k, v in sorted(counts.items())) or "—"

    lines = [f"### `{crate}` (v{version})", ""]
    lines.append(
        f"**{total} public items** — {len(stable)} stable, "
        f"{len(experimental)} experimental"
        + (f", {len(vendored)} vendored-upstream" if vendored else "")
        + f". Categories: {count_s}.")
    lines.append("")

    rules = EXPERIMENTAL_RULES.get(crate, [])
    if rules:
        whys = sorted({w for _p, w in rules})
        lines.append("Experimental areas: " + "; ".join(whys) + ".")
        lines.append("")
    if CRATE_TIER.get(crate) == "VENDORED":
        lines.append("Vendored upstream fork: the entire surface tracks its "
                     "upstream project and is EXPERIMENTAL until the "
                     "maintenance policy in `API_FREEZE_AUDIT.md` §5 pins a "
                     "re-sync contract.")
        lines.append("")
    elif CRATE_TIER.get(crate) == "EXPERIMENTAL":
        lines.append("Whole crate classified EXPERIMENTAL (young integration "
                     "surface, expected to settle during the RC line).")
        lines.append("")

    for caveat in whole_crate_alias_caveats(items):
        lines.append(f"Caveat: {caveat}")
        lines.append("")

    def fmt(path: str) -> str:
        i = items[path]
        tag = {"STABLE": "stable", "EXPERIMENTAL": "experimental",
               "VENDORED": "vendored"}[cls[path]]
        feat = f", feature `{i['feature']}`" if i["feature"] else ""
        kind = KIND_LABEL[i["kind"]]
        return f"- `{path}` — {kind}, {tag}{feat}"

    if total <= LIST_LIMIT:
        for path in sorted(items):
            lines.append(fmt(path))
    else:
        by_mod: dict[str, list[str]] = defaultdict(list)
        for path in items:
            by_mod[module_of(path)].append(path)
        lines.append(f"Large surface — digest by module "
                     f"(> {LIST_LIMIT} items):")
        lines.append("")
        lines.append("| Module | Items | Stable | Experimental | Kinds |")
        lines.append("| :--- | ---: | ---: | ---: | :--- |")
        for mod in sorted(by_mod):
            members = by_mod[mod]
            n_stable = sum(1 for p in members if cls[p] == "STABLE")
            n_exp = sum(1 for p in members if cls[p] == "EXPERIMENTAL")
            n_ven = sum(1 for p in members if cls[p] == "VENDORED")
            kinds = ", ".join(
                f"{v} {k}" for k, v in sorted(
                    Counter(KIND_LABEL[items[p]["kind"]] for p in members)
                    .items()))
            exp_s = f"{n_exp}" + (f" (+{n_ven} vendored)" if n_ven else "")
            lines.append(f"| `{mod}` | {len(members)} | {n_stable} | "
                         f"{exp_s} | {kinds} |")
        lines.append("")
        lines.append("<details><summary>Full item list</summary>")
        lines.append("")
        for path in sorted(items):
            lines.append(fmt(path))
        lines.append("")
        lines.append("</details>")

    if deprecated:
        lines.append("")
        lines.append("Deprecated items: "
                     + ", ".join(f"`{d}`" for d in deprecated))
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--skip-docgen", action="store_true",
                    help="reuse existing target/doc/*.json")
    ap.add_argument("--strict", action="store_true",
                    help="exit nonzero if any crate fails doc generation")
    args = ap.parse_args()

    crates = cargo_metadata()
    print(f"{len(crates)} publishable crates under crates/")

    per_crate: dict[str, dict[str, dict]] = {}
    deprecated: dict[str, list[str]] = {}
    failed: list[str] = []

    def merge_into(merged: dict[str, dict], json_path: Path,
                   feature: str | None) -> None:
        got, meta = load_items(json_path)
        for path, kind in got.items():
            if path in merged:
                continue
            m = meta.get(path, {})
            merged[path] = {"kind": kind, "feature": feature,
                            "src": m.get("src"),
                            "qsrc": m.get("qsrc"),
                            "qsrc_crate": m.get("qsrc_crate"),
                            "target": m.get("target"),
                            "target_crate": m.get("target_crate")}

    for c in crates:
        name = c["name"]
        merged: dict[str, dict] = {}

        if args.skip_docgen:
            default_json = TARGET_DOC / (c["lib"] + ".json")
            if not default_json.exists():
                default_json = None
        else:
            default_json = rustdoc_json(c, None)

        if default_json is None:
            failed.append(name)
            per_crate[name] = {}
            continue

        merge_into(merged, default_json, None)
        deprecated[name] = load_deprecated(default_json)

        feats = EXTRA_FEATURES.get(name)
        if feats and not args.skip_docgen:
            feat_json = rustdoc_json(c, feats)
            if feat_json is not None:
                merge_into(merged, feat_json, feats)
        print(f"  {name}: {len(merged)} items")
        per_crate[name] = merged

    # ---- render -------------------------------------------------------------
    out = [BEGIN, "",
           f"_{len(per_crate)} publishable crates. Generated by "
           "`python3 scripts/api_surface_audit.py` from rustdoc JSON "
           "(stable + `RUSTC_BOOTSTRAP`); do not edit between the markers._",
           ""]
    grand = sum(len(v) for v in per_crate.values())
    n_cls = Counter(classify(c, p, i)
                    for c, items in per_crate.items()
                    for p, i in items.items())
    out.append(f"**Workspace total: {grand} public items, "
               f"{n_cls['STABLE']} stable / {n_cls['EXPERIMENTAL']} "
               f"experimental / {n_cls['VENDORED']} vendored.**")
    out.append("")
    out.append("| Crate | Items | Stable | Experimental | Vendored | Notes |")
    out.append("| :--- | ---: | ---: | ---: | ---: | :--- |")
    for c in crates:
        name = c["name"]
        items = per_crate.get(name, {})
        if name in failed:
            out.append(f"| `{name}` | — | — | — | — | doc generation "
                       "failed on this host |")
            continue
        c_cls = Counter(classify(name, p, i) for p, i in items.items())
        note = {"VENDORED": "vendored fork",
                "EXPERIMENTAL": "crate-level experimental"}.get(
                    CRATE_TIER.get(name), "")
        if whole_crate_alias_caveats(items):
            note = (note + "; " if note else "") + \
                "crate-root aliases expose experimental sub-surface"
        out.append(f"| `{name}` | {len(items)} | {c_cls['STABLE']} | "
                   f"{c_cls['EXPERIMENTAL']} | {c_cls['VENDORED']} | "
                   f"{note} |")
    out.append("")

    for c in crates:
        name = c["name"]
        if name in failed:
            out.append(f"### `{name}` (v{c['version']})")
            out.append("")
            out.append("Rustdoc generation failed on this host; enumerate "
                       "on a host with the crate's system dependencies.")
            out.append("")
            continue
        out.append(emit_crate(name, c["version"], per_crate[name],
                              deprecated.get(name, [])))
    out.append(END)

    doc = DOC_OUT.read_text() if DOC_OUT.exists() else ""
    block = "\n".join(out)
    if BEGIN in doc and END in doc:
        doc = re.sub(re.escape(BEGIN) + ".*?" + re.escape(END),
                     lambda _m: block, doc, flags=re.S)
    else:
        doc = doc.rstrip() + "\n\n" + block + "\n"
    DOC_OUT.write_text(doc)
    print(f"wrote {DOC_OUT}")
    if failed:
        print("FAILED crates:", ", ".join(failed), file=sys.stderr)
        return 1 if args.strict else 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
