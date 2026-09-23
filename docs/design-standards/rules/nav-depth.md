# nav-depth

**Standard(s):** [isa-101](../standards/isa-101.md), [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** deterministic — this is a counted fact about the scene tree, not a judgment call.

## What it measures

Counts nested *navigation-kind* layers on each root→node path in the
scene. A navigation node whose parent is not navigation starts a new
layer; a `Tabs` widget containing a `TabBar` containing `Tab`s counts
as **one** run — the tab strip is a single orientation device, not
three.

Each layer is a separate "where am I" mechanism the user must
maintain. A title bar over tabs over a filter rail over a sidebar
reads as four stacked navigation systems; the user re-orients at every
boundary.

The finding anchors at the node where the depth is exceeded — the
*newest* stacked layer, which is the one to remove or merge.

## The evidence

ISA-101's four-level progressive display hierarchy (L1 overview → L4
diagnostic) exists because operators demonstrably lose the thread when
orientation devices stack — post-incident analyses in the High
Performance HMI literature attribute slow abnormal-situation detection
to exactly this pattern. The general-UI statement is Nielsen's
heuristic #8 (aesthetic and minimalist design): every navigation layer
competes with content for attention.

Working memory is the mechanism ([hci-laws](../standards/hci-laws.md)):
each stacked nav layer is context the user holds while deciding where
to click. Cowan's 4±1 chunk limit is consumed by navigation alone at
high depth.

**Default `max = 2`** — one global orientation device (the app's nav
shell) plus one local one (tabs/a rail inside the current page). A
third stacked layer is where "where am I" complaints reliably begin.

## Configuration

```toml
[rules.nav-depth]
severity = "warn"
max = 2    # maximum stacked navigation layers on any scope path
```

## How to fix

- **Merge layers.** A filter row that only scopes the current tab is a
  control cluster, not navigation — reclassify it (`[classify]`) or
  rename it so it doesn't parse as a nav device.
- **Flatten.** Move the innermost strip's actions into a toolbar or
  overflow menu attached to the content it scopes.
- **Replace a layer with disclosure.** An [Expander or Sheet is a
  pull-down, not a layer](progressive-disclosure.md) — it doesn't
  persistently occupy orientation space.
- **Check `[classify]` first.** If a flagged node is genuinely content
  (e.g. a `Sidebar` showing document metadata), fix the classification
  rather than the layout.

## When it's OK to allow

- **Wizards and multi-step flows** legitimately stack a stepper inside
  a page inside the shell — the depth *is* the feature.
- **Debug/inspection tools** embedded in the app shell.

```toml
[[allow]]
path = "App/SetupWizard/**"
rules = ["nav-depth"]
```

Or inline on the widget: `"SetupWizard@lint:nav-depth"` — inherited by
the whole wizard subtree.
