# tab-count

**Standard(s):** [consistency](../standards/consistency.md), [hci-laws](../standards/hci-laws.md)
**Default severity:** `warn`
**Confidence:** deterministic count on a named idiom — tabs are detected by name; the 7 budget is the Hick's-law surface.

## What it measures

Children named `*tab*` counted per parent; over
`max_tabs` flags.

## The evidence

- Hick's law on a fixed idiom — past ~7 a tab strip is
  a menu pretending to be tabs.

## Configuration

```toml
[rules.tab-count]
severity = "warn"
max_tabs = 7
```

## How to fix

- Group tabs or overflow the tail into a 'More' menu.

## Legitimate exceptions

- Document-style editors (file tabs) where the strip IS
  the working set — allow.
