# text-min-size

**Standard(s):** [wcag](../standards/wcag.md), [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — WCAG sets no minimum size; 9pt is the practical floor below which text is decorative, not readable.

## What it measures

Any `TextStat` smaller than `min_pt` (converted through
`scale_factor`) is flagged. Zero-size runs are ignored.

## The evidence

- **WCAG 1.4.4 Resize Text** presumes a usable base size —
  200% zoom on 6pt text is still 12pt.
- **Legibility research**: sub-9pt body text drops below reading
  thresholds for a large share of users under real conditions.

## Configuration

```toml
[rules.text-min-size]
severity = "info"
min_pt = 9
```

## How to fix

- The autofix (risky) raises the run to `min_pt`.
- If it's truly decorative (a watermark), suppress — but consider
  whether decoration needs paint at all.

## Legitimate exceptions

- Captions/footers are *intended* small but rarely below 9pt;
  axis tick labels are the common legitimate exception.
