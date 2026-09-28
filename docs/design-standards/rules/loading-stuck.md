# loading-stuck

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — the consecutive-sample count is measured,
but "still loading" versus "legitimately slow fetch" is a judgement
call; treat the finding as a review prompt.

## What it measures

Whether a node's skeleton placeholder survives across `min_frames`
consecutive sampled frames — ADR-0040 §5's "stuck loading is lint
policy, not runtime timers" audit, mirroring the arena-level
`audit_underflow` pass.

`LintScene` is single-frame and rules are stateless, so persistence
arrives as data on the scope name, following the existing
`Name@marker` lineage convention:

- `Feed@loading:N` — the node was observed in the loading state for
  N consecutive sampled frames.
- `Feed@loading` — a single sighting (count 1). Never a finding on
  its own: loading is legitimate; *stuck* is the smell.

[`LoadingTracker`](martensite-design-lint's `loading` module) is the
sweep-side probe that produces the counts — `sample_arena(&arena)`
reads the resolved `effective_loading` state (`NodeFlags::LOADING` |
`Widget::is_loading`) once per frame, and `annotate(&mut scene)`
stamps `loading:N` onto the scene's nodes via the `PushScope`
widget-id join. Streams that emit `@loading`/`@loading:N` natively
need no tracker for detection — only for the count.

## The evidence

- **Nielsen heuristic #1, visibility of system status** — a skeleton
  that never resolves reports "pending" forever; the user cannot
  tell a slow fetch from a broken one, and neither can a later
  reader of the screen.
- **ADR-0040 §5** — forgotten `set_loading(false)` and silently
  failed fetches are the two bugs this rule exists to catch; a
  runtime timer would race real latencies, a sampled audit never
  does.

## Configuration

```toml
[rules.loading-stuck]
severity = "warn"
min_frames = 3    # consecutive sampled frames before flagging
```

At the ~2 Hz paint-audit cadence the default is ≈1.5 s of continuous
shimmer — past one full sweep period (`SWEEP_SECS` = 1.4 s). In a
lint sweep it means the node was still loading on the third
consecutive sample.

## How to fix

- Check the `set_loading(false)` / `AsyncState::Ready` transition —
  the skeleton is stuck because the resolve path never ran.
- A fetch that never completes should reach `AsyncState::Error` and
  paint a `ResultPage`, not shimmer indefinitely.

## Legitimate exceptions

- A deliberately perpetual placeholder or a certified "data unknown"
  treatment declares `@skeleton` on the scope name (or an ancestor —
  the marker is inherited by lineage) and is exempt.
- `@lint:loading-stuck` and path `[[allow]]`s suppress normally for
  long-but-legitimate loads (batch imports, slow serial links).
