#!/usr/bin/env python3
"""Single source of truth for workspace-derived CI/release enumerations.

The publish order, the publishable-crate set, and every CI matrix that
enumerates crates are DERIVED from `cargo metadata` — no hand-maintained
lists. This script is both the generator (emit modes) and the gate
(check mode): the same dependency graph feeds both, so they cannot
drift apart.

Subcommands (run from the repository root):

    publish-order   topo-sorted publishable crates, one per line
    publishable     publishable crates (unsorted), one per line
    semver-checks   crates with a checkable API surface: publishable,
                    has a lib target, and tracks the workspace version
                    (excludes proc-macro crates and independently-
                    versioned vendored forks)
    doctest-group <i> <n>
                    workspace members with a lib target assigned to
                    doctest shard i of n (deterministic, name-sorted mod).
                    Excludes the `martensite` facade — its ~4000 doctests
                    are sharded at file granularity instead.
    facade-doctest-group <i> <n>
                    doctest file filters for the `martensite` facade,
                    shard i of n (deterministic, path-sorted mod). Each
                    emitted token is a `src/...` substring that matches
                    exactly one source file's doctest names.
    check           verify a CRATES list on stdin is topological +
                    complete + predicate-clean
    check-workflow  verify every CRATES=(...) array in publish.yml

Predicate (publishable-set gate): a crate that cargo would publish
(`publish` absent or non-empty) must either be named `martensite-*` —
the workspace convention — or carry `package.metadata.ci.publishable
= true` in its manifest. This catches a forgotten `publish = false`
on non-conventional names without encoding a per-crate roster.
"""

import json
import os
import re
import subprocess
import sys

WORKFLOW = ".github/workflows/publish.yml"


def workspace_packages():
    cmd = ["cargo", "metadata", "--format-version", "1", "--no-deps"]
    # `cargo metadata` needs none of rust-toolchain.toml's components or
    # targets — but the rustup shim fetches them on first invocation, and
    # its on-demand download can race its own .partial rename on runners.
    # On CI (where dtolnay installs plain `stable`) bypass the shim
    # entirely; locally use the pinned toolchain, falling back to the
    # bypass if the shim fetch fails.
    orders = (
        [{"RUSTUP_TOOLCHAIN": "stable"}, {}]
        if os.environ.get("CI")
        else [{}, {"RUSTUP_TOOLCHAIN": "stable"}]
    )
    out = None
    for extra in orders:
        try:
            out = subprocess.check_output(
                cmd, env=dict(os.environ, **extra)
            )
            break
        except subprocess.CalledProcessError:
            continue
    if out is None:
        raise SystemExit("cargo metadata failed under both toolchains")
    meta = json.loads(out)
    return {
        p["name"]: p
        for p in meta["packages"]
        if p["id"] in meta["workspace_members"]
    }


def is_publishable(pkg) -> bool:
    publish = pkg.get("publish")
    return publish is None or bool(publish)


def predicate_clean(name, pkg) -> bool:
    """Publishable crates must follow the martensite-* convention or
    explicitly opt in via package metadata."""
    if not is_publishable(pkg):
        return True
    if name.startswith("martensite"):
        return True
    return bool(
        (pkg.get("metadata") or {}).get("ci", {}).get("publishable") is True
    )


def publishable_set(ws):
    return {n for n, p in ws.items() if is_publishable(p)}


def workspace_deps(pkg, within):
    """Workspace dependency edges restricted to `within` — includes
    normal, build, target-specific, optional, and versioned dev-deps:
    all of them must resolve on the index at publish time."""
    out = set()
    for dep in pkg["dependencies"]:
        if dep.get("path") and dep["name"] in within:
            out.add(dep["name"])
    return out


def topo_order(ws, names):
    """Kahn's algorithm, name-sorted tie-break for determinism."""
    deps = {n: workspace_deps(ws[n], names) for n in names}
    indeg = {n: len(deps[n]) for n in names}
    ready = sorted(n for n in names if indeg[n] == 0)
    out = []
    while ready:
        n = ready.pop(0)
        out.append(n)
        for m in names:
            if n in deps[m]:
                indeg[m] -= 1
                if indeg[m] == 0:
                    ready.append(m)
                    ready.sort()
    if len(out) != len(names):
        raise SystemExit("dependency cycle in publishable set")
    return out


def check_order(order, ws, label):
    pos = {c: i for i, c in enumerate(order)}
    failures = 0
    publishable = publishable_set(ws)

    for name in order:
        if name not in ws:
            print(f"{label}: '{name}' is not a workspace member", file=sys.stderr)
            failures += 1
            continue
        if name not in publishable:
            print(
                f"{label}: '{name}' is publish = false — cannot publish",
                file=sys.stderr,
            )
            failures += 1
            continue
        for dep in ws[name]["dependencies"]:
            d = dep["name"]
            if dep.get("path") and d in pos and pos[d] > pos[name]:
                print(
                    f"{label}: {name} (pos {pos[name]}) before its "
                    f"dependency {d} (pos {pos[d]})",
                    file=sys.stderr,
                )
                failures += 1

    for name in sorted(publishable):
        if name not in pos:
            print(
                f"{label}: publishable crate '{name}' missing from list",
                file=sys.stderr,
            )
            failures += 1

    for name, pkg in sorted(ws.items()):
        if not predicate_clean(name, pkg):
            print(
                f"{label}: publishable crate '{name}' fails the predicate "
                f"(name must match martensite-* or set "
                f"package.metadata.ci.publishable = true)",
                file=sys.stderr,
            )
            failures += 1
    return failures


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    cmd = sys.argv[1]
    ws = workspace_packages()

    if cmd == "publishable":
        for n in sorted(publishable_set(ws)):
            print(n)
        return 0

    if cmd == "publish-order":
        for n in topo_order(ws, publishable_set(ws)):
            print(n)
        return 0

    if cmd == "semver-checks":
        ws_version = ws["martensite"]["version"]
        for n in sorted(publishable_set(ws)):
            pkg = ws[n]
            kinds = {k for t in pkg["targets"] for k in t["kind"]}
            # Vendored forks carry upstream code — semver gates belong to
            # upstream's release cadence, not ours (marked via
            # package.metadata.ci.vendored; independently-versioned
            # forks are also excluded by the ws_version check).
            if (
                "lib" not in kinds
                or pkg["version"] != ws_version
                or (pkg.get("metadata") or {}).get("ci", {}).get("vendored")
            ):
                continue
            print(n)
        return 0

    if cmd == "doctest-group":
        i, n = int(sys.argv[2]), int(sys.argv[3])
        # `lib` OR `proc-macro` targets can carry doctests — martensite-
        # macros documents its macros with real ``` blocks, and rustdoc
        # compiles proc-macro doctests downstream of the crate.
        libs = sorted(
            name
            for name, pkg in ws.items()
            if name != "martensite"
            and any(
                any(k in ("lib", "proc-macro") for k in t["kind"])
                for t in pkg["targets"]
            )
        )
        for idx, name in enumerate(libs):
            if idx % n == i - 1:
                print(name)
        return 0

    if cmd == "facade-doctest-group":
        i, n = int(sys.argv[2]), int(sys.argv[3])
        src = os.path.join(
            os.path.dirname(ws["martensite"]["manifest_path"]), "src"
        )
        files = sorted(
            os.path.relpath(os.path.join(root, f), os.path.dirname(src))
            for root, _, names in os.walk(src)
            for f in names
            if f.endswith(".rs")
        )
        for idx, rel in enumerate(files):
            if idx % n == i - 1:
                # Filters are substring-matched against doctest names
                # (`crates/martensite/src/x.rs - item::path (line N)`),
                # and the `.rs` suffix already prevents prefix collisions
                # (`button.rs` cannot match `button_group.rs`). Do NOT
                # add whitespace to these filters: rustdoc/libtest split
                # test-args on spaces, so `x.rs - ` becomes two filters
                # and the lone `-` matches every doctest name.
                print(rel)
        return 0

    if cmd == "check":
        order = [l.strip() for l in sys.stdin if l.strip()]
        f = check_order(order, ws, "stdin")
        print(f"check: {f} violation(s)" if f else "check: OK")
        return 1 if f else 0

    if cmd == "check-workflow":
        text = open(WORKFLOW).read()
        lists = re.findall(r"CRATES=\(\s*(.*?)\)", text, re.S)
        if not lists:
            print("no CRATES lists found in publish.yml", file=sys.stderr)
            return 1
        failures = 0
        for idx, raw in enumerate(lists):
            order = [l.strip() for l in raw.split("\n") if l.strip()]
            failures += check_order(order, ws, f"list {idx}")
        if failures:
            print(f"{failures} publish-order violation(s)", file=sys.stderr)
            return 1
        print(f"publish order OK ({len(lists)} list(s), {len(order)} crates)")
        return 0

    print(f"unknown subcommand: {cmd}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
