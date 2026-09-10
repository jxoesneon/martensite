#!/usr/bin/env bash
set -euo pipefail

RESULTS_DIR="${MARTENSITE_FUZZ_RESULTS_DIR:-/tmp}"
mkdir -p "$RESULTS_DIR"
RESULTS_FILE="$RESULTS_DIR/fuzz-results.txt"

SEED="${MARTENSITE_FUZZ_SEED:-1}"
DURATION="${MARTENSITE_FUZZ_DURATION:-172800}"
RELEASE_FLAG=""
if [ "${MARTENSITE_FUZZ_RELEASE:-1}" = "1" ]; then
    RELEASE_FLAG="--release"
fi

hours=$((DURATION / 3600))
mins=$(((DURATION % 3600) / 60))

{
    echo "=== Martensite Fuzz Soak Campaign ==="
    echo "Seed: $SEED"
    echo "Duration: ${DURATION}s (${hours}h ${mins}m)"
    echo "Profile: ${RELEASE_FLAG:-dev}"
    echo "Started: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo ""
    echo "Building fuzz test binary..."
} > "$RESULTS_FILE"

cargo test -p martensite-test --lib $RELEASE_FLAG --no-run 2>&1 | tee -a "$RESULTS_FILE"

echo "" | tee -a "$RESULTS_FILE"
echo "Running soak campaign..." | tee -a "$RESULTS_FILE"

# Hourly progress marker so the log is not silent for the full duration.
(
    while true; do
        echo "[progress] $(date -u '+%Y-%m-%dT%H:%M:%SZ') - campaign still running"
        sleep 3600
    done
) >> "$RESULTS_FILE" &
PROGRESS_PID=$!

set +e
cargo test -p martensite-test --lib $RELEASE_FLAG soak_campaign -- --ignored --nocapture 2>&1 | tee -a "$RESULTS_FILE"
STATUS=${PIPESTATUS[0]}
set -e

kill "$PROGRESS_PID" 2>/dev/null || true

echo "" | tee -a "$RESULTS_FILE"
echo "Finished: $(date -u '+%Y-%m-%dT%H:%M:%SZ')" | tee -a "$RESULTS_FILE"

if [ "$STATUS" -eq 0 ]; then
    echo "Result: PASS" | tee -a "$RESULTS_FILE"
    exit 0
else
    echo "Result: FAIL (exit code $STATUS)" | tee -a "$RESULTS_FILE"
    exit "$STATUS"
fi
