# menu-depth

**Standard(s):** [hci-laws](../standards/hci-laws.md), [consistency](../standards/consistency.md)
**Default severity:** `warn`
**Confidence:** deterministic — nested menu-named depth is measured.

## What it measures

The longest menu→submenu *chain* hanging off each menu —
menu-item leaves and non-menu wrappers don't count. Depth
past `max_depth` (default 2) flags at the outermost menu.

## The evidence

- Fitts's law on narrowing corridors — each cascade
  level shrinks the steering tunnel; accidental dismissal resets the
  whole path.

## Configuration

```toml
[rules.menu-depth]
severity = "warn"
max_depth = 2   # menu→submenu chain length over this flags
```

## How to fix

- Flatten deep cascades; two levels is the practical ceiling.

## Legitimate exceptions

- Deep taxonomies (layer trees) — allow.
