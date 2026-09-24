# flood-cap

**Standard(s):** [isa-18-2](../standards/isa-18-2.md)
**Default severity:** `warn`
**Confidence:** heuristic — the 10-alarms/10-min flood threshold has no exact design-time analog; 5 simultaneous alert elements is the conservative proxy.

## What it measures

Per surface, every descendant painting in the alarm-red
family counts as one simultaneous alert. Over `max_simultaneous` =
finding.

## The evidence

- **ISA-18.2**: attention is a single channel — the
  flood threshold exists because N alerts at once becomes zero alerts
  noticed.

## Configuration

```toml
[rules.flood-cap]
severity = "warn"
max_simultaneous = 5
```

## How to fix

- Rationalize: aggregate sub-alarms into summary states,
  gate low-priority alerts behind a drill-down.

## Legitimate exceptions

- The alarm summary page itself legitimately shows many
  alerts — allow `**/AlarmList`.
