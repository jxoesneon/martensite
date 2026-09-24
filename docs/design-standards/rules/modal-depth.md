# modal-depth

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — modal detection is name-based (modal|dialog|overlay|sheet|popover).

## What it measures

Nested modal-named nodes beyond depth 1 flag.

## The evidence

- Attention is single-threaded — stacked modals strand
  context and make the escape path ambiguous (which does Esc close?).

## Configuration

```toml
[rules.modal-depth]
severity = "warn"
# thresholds: >1 stacked modal layer flags (fixed)
```

## How to fix

- Flatten to one modal, or convert the inner to inline
  confirmation.

## Legitimate exceptions

- System-critical confirm-over-dialog — allow.
