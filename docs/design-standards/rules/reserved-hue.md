# reserved-hue

**Standard(s):** [isa-101](../standards/isa-101.md)
**Default severity:** `error`
**Confidence:** deterministic — the red family is a measured hue range; the only judgment is the `@alarm` lineage check.

## What it measures

Every fill and text color outside an `@alarm` lineage is
checked against the alarm-red family (saturated red/orange-magenta).
Any hit = finding.

## The evidence

- **ISA-101 / ASM Consortium**: saturated red is the
  abnormal-state channel — full stop. Decorative red trains operators
  to ignore the exact color a real alarm uses.

## Configuration

```toml
[rules.reserved-hue]
severity = "error"
# no thresholds — alarm-red family detection is fixed;
# exempt lineages carry @alarm
```

## How to fix

- The autofix (risky) desaturates toward gray. For legitimate
  alarm paint, add `@alarm` to the scope's `debug_name` instead of
  recoloring.

## Legitimate exceptions

- Brand-red logos are the classic false positive — this is
  why `error` not `forbid`; allow the specific path.
