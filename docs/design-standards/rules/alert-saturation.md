# alert-saturation

**Standard(s):** [isa-18-2](../standards/isa-18-2.md), [isa-101](../standards/isa-101.md)
**Default severity:** `warn`
**Confidence:** heuristic (deterministic-ish) — the signals counted are
concrete (alert-named scopes and alarm-colored fills), but whether N
simultaneous alerts constitutes a "flood" is a design judgment. Counted
like a fact, interpreted like a prompt.

## What it measures

Counts **simultaneous alert-level signals** per surface: scopes whose
names indicate alerts/alarms/criticals, plus leaf subtrees dominated by
saturated alarm-red fills. More than a small budget of these on one
screen is the design-time analog of an alarm flood — everything is
screaming, so nothing is.

## The evidence

- **ANSI/ISA-18.2** ([isa-18-2](../standards/isa-18-2.md)): alarm floods
  destroy response. The standard's published rates — ~150 alarms/day
  acceptable, **>10 alarms per 10 minutes unmanageable**, operators
  effectively triaging ~1–2 alarms/10 min — quantify the ceiling on
  simultaneous urgency. The screen analog: beyond a few simultaneous
  alert signals, each additional one degrades *all* of them.
- **WCAG 1.4.1** ([wcag](../standards/wcag.md)): a screen saturated
  with alert color is also a use-of-color failure — the signal is
  hue-only and now indistinguishable even for color-typical users.
- **Endsley's situation-awareness model** ([isa-101](../standards/isa-101.md)):
  alert floods break SA level 1 — perception — before comprehension is
  even possible.

**Default `max = 3`** — three simultaneous alerts is already an
incident's worth of attention. This is a design-time analog bound, not
a number ISA-18.2 publishes for screens.

## Configuration

```toml
[rules.alert-saturation]
severity = "warn"
max = 3               # max simultaneous alert signals per surface
min_surface_pt = 20000  # surfaces below this area are skipped
```

## How to fix

- **Prioritize.** ISA-18.2's core doctrine is priority distribution —
  a handful of criticals, not a democracy. If five things are alerting
  at once, the design question is which three aren't actually urgent.
- **De-escalate stale signals.** A "critical" badge that has been red
  for a week is noise, not an alert — demote it to a muted state.
- **Aggregate.** N warnings in one region are one "3 issues" summary
  control, not N red fills.
- **Check the color channel.** If the signal is a saturated fill with
  no text, [color-only-info](color-only-info.md) will likely fire too —
  fix both by adding redundant coding.

## When it's OK to allow

- **Alarm-list surfaces** — an alarm console is *supposed* to show
  many alerts; that's its function, not a flood.
- **Incident/war-room dashboards** during a drill or live event.

```toml
[[allow]]
path = "App/AlarmConsole/**"
rules = ["alert-saturation"]
```
