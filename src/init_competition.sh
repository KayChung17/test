#!/bin/sh

export HOME=/

# Read skip list from file if present (for local testing)
[ -f /etc/skip_suites ] && SKIP_SUITES=$(cat /etc/skip_suites)

TEST_DIR="/oscomp/glibc"
LTPROOT="$TEST_DIR/ltp"

if [ ! -d "$TEST_DIR" ]; then
    echo "=== Starry OS ==="
    echo "Test dir not found: $TEST_DIR"
    cd /root
    exec sh --login
fi

echo "=== Starry OS Competition Mode ==="
echo "Test dir: $TEST_DIR"

# ---- glibc dynamic linker setup ----
GLIBC_LIB="$TEST_DIR/lib"
ln -sf "$GLIBC_LIB/ld-linux-riscv64-lp64d.so.1" /lib/
ln -sf "$GLIBC_LIB/libc.so.6" /lib/
ln -sf "$GLIBC_LIB/libm.so.6" /lib/

cd "$TEST_DIR"

# Avoid pager pauses during verbose test output.
stty rows 1000 cols 200 >/dev/null 2>&1 || true
export PAGER=cat
export TERM=dumb

# ---- scan for test entry points ----
SCRIPTS=$(ls *_testcode.sh 2>/dev/null | sort)

if [ -z "$SCRIPTS" ]; then
    echo "[SUMMARY] no testcode scripts found in $TEST_DIR"
    echo "=== tests done, powering off ==="
    exit 0
fi

SUITE_PASS=0
SUITE_FAIL=0
SUITE_SKIP=0

# ---- bench-type recognition helpers ----
is_aggregate() {
    # aggregate = script-driven batch bench whose run-all.sh lives in
    # the top-level glibc dir (not in a subdirectory). Currently only
    # libc-test uses this pattern: run-all.sh + entry-*.exe + runtest.exe
    local name="$1"
    [ "$name" = "libctest" ] && \
    [ -x ./run-all.sh ] && [ -x ./entry-static.exe ] && \
    [ -x ./entry-dynamic.exe ] && [ -x ./runtest.exe ]
}

is_directory_scan() {
    # directory-scan: has a subdirectory with its own run-all.sh
    # (basic/, ltp/, etc.)
    local dir="$1"
    [ -d "./$dir" ] && [ -x "./$dir/run-all.sh" ]
}

for script in $SCRIPTS; do
    name="${script%_testcode.sh}"
    echo "[SUITE-BEGIN] $name"

    # Skip suites listed in SKIP_SUITES (comma-separated)
    if echo ",$SKIP_SUITES," | grep -q ",$name,"; then
        echo "[SUITE-SKIP] $name (skipped by SKIP_SUITES)"
        SUITE_SKIP=$((SUITE_SKIP + 1))
        echo "[SUITE-END] $name"
        continue
    fi

    if [ ! -x "$script" ]; then
        echo "[SUITE-SKIP] $name (not executable)"
        SUITE_SKIP=$((SUITE_SKIP + 1))
        echo "[SUITE-END] $name"
        continue
    fi

    # recognize bench type
    if [ "$name" = "ltp" ] && [ -x "$LTPROOT/runltp" ]; then
        echo "[SUITE-TYPE] directory-scan"
        export LTPROOT
        export PATH="$LTPROOT/testcases/bin:$PATH"
        # Pre-generate alltests file using input redirection
        # (BusyBox sh's "cat | while read" pipe doesn't propagate writes)
        LTP_TMP="/tmp/ltp-run-$$"
        mkdir -p "$LTP_TMP"
        LTP_ALLTESTS="$LTP_TMP/alltests"
        while read scenfile; do
            f="$LTPROOT/runtest/$scenfile"
            [ -f "$f" ] && cat "$f" >> "$LTP_ALLTESTS"
        done < "$LTPROOT/scenario_groups/default"
        echo "[LTP] alltests: $(wc -l < "$LTP_ALLTESTS" 2>/dev/null) lines"
        cd "$LTPROOT" || exit 1
        mkdir -p "$LTPROOT/output" "$LTPROOT/results"
        "$LTPROOT/bin/ltp-pan" -e -S -O "$LTP_TMP" -a $$ -n $$ -f "$LTP_ALLTESTS"
        rc=$?
        cd "$TEST_DIR" || exit 1
        rm -rf "$LTP_TMP"
    else
        if is_aggregate "$name"; then
            echo "[SUITE-TYPE] aggregate"
        elif is_directory_scan "$name"; then
            echo "[SUITE-TYPE] directory-scan"
        else
            echo "[SUITE-TYPE] standalone"
        fi

        /bin/sh "$script"
        rc=$?
    fi

    echo "[SUITE-RESULT] $name exit=$rc"
    if [ "$rc" -eq 0 ]; then
        SUITE_PASS=$((SUITE_PASS + 1))
    else
        SUITE_FAIL=$((SUITE_FAIL + 1))
    fi
    echo "[SUITE-END] $name"
done

echo "[SUMMARY] suites=$((SUITE_PASS + SUITE_FAIL + SUITE_SKIP)) pass=$SUITE_PASS fail=$SUITE_FAIL skip=$SUITE_SKIP"
echo "=== tests done, powering off ==="
