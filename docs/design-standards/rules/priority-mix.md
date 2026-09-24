# priority-mix

**Standard(s):** [isa-18-2](../standards/isa-18-2.md)
**Default severity:** `info`
**Confidence:** heuristic — 80/15/5 is a rationalization norm for alarm systems; a display's declared priorities should rhyme with it.

## What it measures

Counts `@priority:N` markers (clamped 1–3). With ≥
`min_alarms` declared, high-priority share over `max_high_pct` =
finding.

## The evidence

- **ISA-18.2 rationalization**: ~80% low / 15% medium /
  5% high. Uniform priority is no priority.

## Configuration

```toml
[rules.priority-mix]
severity = "info"
max_high_pct = 10
min_alarms = 4
```

## How to fix

- Re-rationalize: demote alerts whose consequence doesn't
  demand immediate operator action.

## Legitimate exceptions

- A genuinely safety-critical subsystem — allow with a
  `reason` field documenting the hazard analysis.
