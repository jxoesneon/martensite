# symmetry

**Standard(s):** [perception](../standards/perception.md)
**Default severity:** `info`
**Confidence:** heuristic — high aesthetic-valence metric but wrong for most functional layouts; default info, not warn.

## What it measures

Mirror-correspondence of child geometry about the
surface's vertical axis.

## The evidence

- Miniukovich & De Angeli — symmetry correlates with
  perceived aesthetics, but deliberate asymmetry is a valid choice;
  this is a review prompt, not a violation.

## Configuration

```toml
[rules.symmetry]
severity = "info"
max_asymmetry = 0.5   # fraction of child mass without a mirror counterpart
```

## How to fix

- If symmetry is the intent (dialogs, forms), align the pair.

## Legitimate exceptions

- Most dashboards are intentionally asymmetric — consider
  turning this off project-wide and enabling for dialog folders only.
