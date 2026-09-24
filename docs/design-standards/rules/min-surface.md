# min-surface

**Standard(s):** [consistency](../standards/consistency.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — the 48pt floor is convention; role-dependent.

## What it measures

Container-kind nodes under `min_pt` on either axis
flag — too small for their implied role.

## The evidence

- A content zone too small for content is dead
  allocation or crammed layout.

## Configuration

```toml
[rules.min-surface]
severity = "info"
min_pt = 48
```

## How to fix

- The autofix (risky) grows bounds to the floor.

## Legitimate exceptions

- Dividers/spacers are small by design — they're not
  Container kind, but if classified as such, allow.
