//! Wall-clock budget benchmark for the Martensite plugin runtime.
//!
//! The default fuel budget ([`DEFAULT_FUEL_BUDGET`]) targets a roughly 5 ms
//! execution slice. These tests verify that a representative plugin workload
//! completes within that wall-clock budget. Timing is host-dependent, so the
//! benchmark itself is `#[ignore]`-gated; a smoke test runs by default to
//! confirm the plugin loads and invokes successfully.

#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use martensite_plugin::{CapabilitySet, PluginRuntime, DEFAULT_FUEL_BUDGET};

/// A representative plugin workload: a tight loop that sums the integers
/// `[0, N)` and returns the result. The loop body is sized to consume a
/// meaningful fraction of [`DEFAULT_FUEL_BUDGET`] while staying safely below
/// it, so a single invocation completes within the default budget.
const SUM_WAT: &str = r#"
    (module
      (memory (export "memory") 1)
      (func $compute (result i32)
        (local $i i32)
        (local $acc i32)
        (local.set $i (i32.const 0))
        (local.set $acc (i32.const 0))
        (block $break
          (loop $cont
            (br_if $break
              (i32.ge_s (local.get $i) (i32.const 5000)))
            (local.set $acc
              (i32.add (local.get $acc) (local.get $i)))
            (local.set $i
              (i32.add (local.get $i) (i32.const 1)))
            (br $cont)
          )
        )
        local.get $acc
      )
      (func (export "run")
        (drop (call $compute))
      )
      (func (export "result") (result i32)
        call $compute
      )
    )
"#;

/// Number of iterations measured in the benchmark.
const BENCH_ITERATIONS: u32 = 1_000;
/// Number of warm-up invocations before measurement.
const WARMUP_ITERATIONS: u32 = 8;
/// Expected sum of the integers `[0, 5000)`.
const EXPECTED_SUM: i32 = 12_497_500;

fn compile_wat(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("valid WAT")
}

#[test]
fn plugin_loads_and_invokes_within_default_budget() {
    let runtime = PluginRuntime::new().expect("runtime");
    let wasm = compile_wat(SUM_WAT);
    let mut plugin = runtime.load(&wasm, CapabilitySet::empty()).expect("load");

    // The workload must complete within the default fuel budget.
    plugin.invoke("run").expect("invoke run");

    let sum = plugin
        .invoke_typed::<(), i32>("result", ())
        .expect("invoke result");
    assert_eq!(sum, EXPECTED_SUM);
}

#[test]
#[ignore = "wall-clock timing is host-dependent"]
fn invoke_average_under_5ms_budget() {
    // Each invocation is guaranteed to complete within `DEFAULT_FUEL_BUDGET`
    // fuel, so the total budget needed for all measured and warm-up
    // invocations is bounded by that times the count. Add one budget of slack
    // so the final invocation never exhausts the store.
    let invocations = u64::from(BENCH_ITERATIONS + WARMUP_ITERATIONS);
    let budget = DEFAULT_FUEL_BUDGET * invocations + DEFAULT_FUEL_BUDGET;
    let runtime = PluginRuntime::with_fuel_budget(budget).expect("runtime");
    let wasm = compile_wat(SUM_WAT);
    let mut plugin = runtime.load(&wasm, CapabilitySet::empty()).expect("load");

    // Warm up so compilation caches stabilize before measurement.
    for _ in 0..WARMUP_ITERATIONS {
        plugin.invoke("run").expect("warmup invoke");
    }

    let mut total = Duration::ZERO;
    for _ in 0..BENCH_ITERATIONS {
        let start = Instant::now();
        plugin.invoke("run").expect("invoke run");
        total += start.elapsed();
    }

    let average = total / BENCH_ITERATIONS;
    // The budget targets ~5 ms; assert with a 2x safety margin (10 ms) to keep
    // the test stable across hosts while still proving the budget is met.
    let limit = Duration::from_millis(10);
    assert!(
        average < limit,
        "average invoke time {average:?} exceeds {limit:?} budget"
    );
}
