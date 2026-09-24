# menu-breadth

**Standard(s):** [hci-laws](../standards/hci-laws.md), [consistency](../standards/consistency.md)
**Default severity:** `warn`
**Confidence:** deterministic count on menu-named nodes.

## What it measures

Menu/dropdown-named nodes with more than `max_items`
children flag.

## The evidence

- Hick's law — flat menus past ~7 force serial scan;
  sectioning is cheap.

## Configuration

```toml
[rules.menu-breadth]
severity = "warn"
max_items = 7
```

## How to fix

- Section the menu or cascade genuinely-secondary items.

## Legitimate exceptions

- Known-item pickers (font menus, timezone) where scan is
  the interaction — allow.
