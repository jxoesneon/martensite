#!/usr/bin/env python3
"""Verify the publish.yml CRATES list is topologically ordered.

Extracts the `CRATES=(...)` arrays from .github/workflows/publish.yml,
builds the workspace dependency graph via `cargo metadata`, and fails
if any crate is listed before a workspace dependency that is itself
published. Also fails if a publishable workspace crate is missing from
the list — a crate absent here would silently ship stale on release.

Run from the repository root:

    python3 scripts/check-publish-order.py
"""

import json
import re
import subprocess
import sys

WORKFLOW = ".github/workflows/publish.yml"


def main() -> int:
    text = open(WORKFLOW).read()
    lists = re.findall(r"CRATES=\(\s*(.*?)\)", text, re.S)
    if not lists:
        print("no CRATES lists found in publish.yml", file=sys.stderr)
        return 1

    meta = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"]
        )
    )
    members = {p["id"]: p for p in meta["packages"]}
    ws = {
        p["name"]: p
        for p in members.values()
        if p["id"] in meta["workspace_members"]
    }

    failures = 0
    for idx, raw in enumerate(lists):
        order = [l.strip() for l in raw.split("\n") if l.strip()]
        pos = {c: i for i, c in enumerate(order)}

        # Every workspace dep published must precede its dependents.
        for name in order:
            pkg = ws.get(name)
            if pkg is None:
                print(
                    f"list {idx}: '{name}' is not a workspace member",
                    file=sys.stderr,
                )
                failures += 1
                continue
            for dep in pkg["dependencies"]:
                dname = dep["name"]
                if dep.get("path") and dname in pos and pos[dname] > pos[name]:
                    print(
                        f"list {idx}: {name} (pos {pos[name]}) published "
                        f"before its dependency {dname} (pos {pos[dname]})",
                        file=sys.stderr,
                    )
                    failures += 1

        # Every publishable martensite crate must be listed — `publish`
        # is either absent (default true) or an array of registries.
        for name, pkg in ws.items():
            publishable = pkg.get("publish") is None or pkg["publish"]
            if publishable and name.startswith("martensite") and name not in pos:
                print(
                    f"list {idx}: publishable crate '{name}' missing from list",
                    file=sys.stderr,
                )
                failures += 1

    if failures:
        print(f"{failures} publish-order violation(s)", file=sys.stderr)
        return 1
    print(f"publish order OK ({len(lists)} list(s), {len(order)} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
