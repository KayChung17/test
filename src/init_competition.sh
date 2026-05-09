#!/bin/sh

export HOME=/root
BENCH_DIR="/oscomp/bench/basic"

# If no bench directory or MODE=debug, drop to shell
if [ ! -d "$BENCH_DIR" ] || [ "$MODE" = "debug" ]; then
    echo "=== Starry OS ==="
    if [ ! -d "$BENCH_DIR" ]; then
        echo "Bench directory not found: $BENCH_DIR"
    fi
    if [ "$MODE" = "debug" ]; then
        echo "Debug mode, entering shell..."
    fi
    cd ~
    exec sh --login
fi

echo "[SUITE-BEGIN] basic"

PASS=0
FAIL=0
tests=$(ls -1 "$BENCH_DIR" 2>/dev/null)
suite_code=0

for name in $tests; do
    dir="$BENCH_DIR/$name"
    [ -d "$dir" ] || continue

    echo "[CASE-BEGIN] $name"

    if [ -f "$dir/run.sh" ]; then
        (cd "$dir" && exec sh run.sh)
        rc=$?
    elif [ -f "$dir/main" ]; then
        (cd "$dir" && exec ./main)
        rc=$?
    else
        rc=127
    fi

    if [ "$rc" -eq 0 ]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        suite_code=1
    fi

    echo "[CASE-END] $name code=$rc"
done

echo "[SUITE-END] basic code=$suite_code"
echo "PASS: $PASS  FAIL: $FAIL"
