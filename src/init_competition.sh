#!/bin/sh
# Competition-mode test runner.
# Scans /oscomp/bench/basic/, runs each test serially, logs results, exits.

echo "=== Starry OS Competition Mode ==="

BENCH_DIR="/oscomp/bench/basic"
PASS=0
FAIL=0
SKIP=0

if [ ! -d "$BENCH_DIR" ]; then
    echo "ERROR: $BENCH_DIR not found"
    exit 1
fi

echo "Scanning $BENCH_DIR ..."
tests=$(ls -1 "$BENCH_DIR" 2>/dev/null)
if [ -z "$tests" ]; then
    echo "No tests found in $BENCH_DIR"
    echo "=== Results: 0 passed, 0 failed ==="
    exit 0
fi

echo ""
for test_name in $tests; do
    test_dir="$BENCH_DIR/$test_name"
    if [ ! -d "$test_dir" ]; then
        continue
    fi

    echo "--- [$test_name] START ---"

    if [ -f "$test_dir/run.sh" ]; then
        (cd "$test_dir" && sh run.sh)
        rc=$?
    elif [ -f "$test_dir/main" ]; then
        (cd "$test_dir" && ./main)
        rc=$?
    else
        echo "[SKIP] $test_name (no run.sh or main)"
        SKIP=$((SKIP + 1))
        continue
    fi

    if [ "$rc" -eq 0 ]; then
        echo "[PASS] $test_name"
        PASS=$((PASS + 1))
    else
        echo "[FAIL] $test_name (exit code: $rc)"
        FAIL=$((FAIL + 1))
    fi
    echo ""
done

echo "=== Results: $PASS passed, $FAIL failed, $SKIP skipped ==="
echo "Competition run complete."
