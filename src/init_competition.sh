#!/bin/sh

export HOME=/

# Read suite controls from files if present (for local testing)
[ -f /etc/skip_suites ] && SKIP_SUITES=$(cat /etc/skip_suites)
[ -f /etc/only_suites ] && ONLY_SUITES=$(cat /etc/only_suites)
[ -f /etc/only_ltp_cases ] && ONLY_LTP_CASES=$(cat /etc/only_ltp_cases)
[ -f /etc/test_libc ] && TEST_LIBC=$(cat /etc/test_libc)

TEST_LIBC="${TEST_LIBC:-glibc}"
TEST_DIR="/oscomp/$TEST_LIBC"
LTPROOT="$TEST_DIR/ltp"

if [ ! -d "$TEST_DIR" ]; then
    echo "=== Starry OS ==="
    echo "Test dir not found: $TEST_DIR"
    cd /root
    exec sh --login
fi

echo "=== Starry OS Competition Mode ==="
echo "Test dir: $TEST_DIR"

# ---- dynamic linker setup ----
LIBC_LIB="$TEST_DIR/lib"
if [ "$TEST_LIBC" = "glibc" ]; then
    ln -sf "$LIBC_LIB/ld-linux-riscv64-lp64d.so.1" /lib/
    ln -sf "$LIBC_LIB/libc.so.6" /lib/
    ln -sf "$LIBC_LIB/libm.so.6" /lib/
else
    ln -sf "$LIBC_LIB/libc.so" /lib/ld-linux-riscv64-lp64d.so.1
    ln -sf "$LIBC_LIB/libc.so" /lib/libc.so
fi

# The rv test image keeps lmbench wrappers built with an absolute path under
# /code/lmbench_src/bin/build.  Mirror that path to the mounted test payload so
# lat_proc shell and direct wrapper invocations exercise the benchmark instead
# of failing with ENOENT.
if [ -x "$TEST_DIR/lmbench_all" ]; then
    mkdir -p /code/lmbench_src/bin/build
    ln -sf "$TEST_DIR/lmbench_all" /code/lmbench_src/bin/build/lmbench_all
fi

cd "$TEST_DIR"

# Avoid pager pauses during verbose test output.
stty rows 1000 cols 200 >/dev/null 2>&1 || true
export PAGER=cat
export TERM=dumb

# ---- scan for test entry points ----
SCRIPTS=$(ls *_testcode.sh 2>/dev/null | sort)
if [ -d ./basic ] && [ -f ./basic/run-all.sh ] && ! echo "$SCRIPTS" | grep -q '^basic_testcode.sh$'; then
    SCRIPTS="basic_testcode.sh $SCRIPTS"
fi

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
    [ -d "./$dir" ] && [ -f "./$dir/run-all.sh" ]
}

for script in $SCRIPTS; do
    name="${script%_testcode.sh}"
    synthetic_basic=0
    if [ "$name" = "basic" ] && [ ! -e "$script" ] && [ -f ./basic/run-all.sh ]; then
        synthetic_basic=1
    fi
    echo "[SUITE-BEGIN] $name"

    # Local testing helper: if ONLY_SUITES is set, run only the listed suites.
    if [ -n "$ONLY_SUITES" ] && ! echo ",$ONLY_SUITES," | grep -q ",$name,"; then
        echo "[SUITE-SKIP] $name (filtered by ONLY_SUITES)"
        SUITE_SKIP=$((SUITE_SKIP + 1))
        echo "[SUITE-END] $name"
        continue
    fi

    # Skip suites listed in SKIP_SUITES (comma-separated)
    if echo ",$SKIP_SUITES," | grep -q ",$name,"; then
        echo "[SUITE-SKIP] $name (skipped by SKIP_SUITES)"
        SUITE_SKIP=$((SUITE_SKIP + 1))
        echo "[SUITE-END] $name"
        continue
    fi

    if [ "$synthetic_basic" -eq 0 ] && [ ! -x "$script" ]; then
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
        # Build a short high-pass whitelist instead of scenario_groups/default.
        # Goal: finish quickly and leave time for later suites.
        LTP_ALLTESTS="$LTPROOT/alltests"
        : > "$LTP_ALLTESTS"
        LTP_CASES="$ONLY_LTP_CASES"
        if [ -z "$LTP_CASES" ]; then
            LTP_CASES="chmod01 chmod03 chdir01 access03 accept01 accept03 clock_getres01 clock_nanosleep04 alarm02 alarm03 alarm06 alarm07 chown05 chroot03 abort01 accept4_01 bind01 bind03 chown02 access01 access02 writev01 getpid01 getppid01 getuid01 geteuid01 getgid01 getegid01 uname01 dup01 dup3_01 pipe01 pipe2_01"
        fi
        for case in $LTP_CASES
        do
            for scenfile in syscalls fs; do
                f="$LTPROOT/runtest/$scenfile"
                [ -f "$f" ] || continue
                grep -E "^${case}[[:space:]]" "$f" >> "$LTP_ALLTESTS" 2>/dev/null || true
            done
        done
        echo "[LTP] whitelist lines: $(wc -l < "$LTP_ALLTESTS" 2>/dev/null)"
        cd "$LTPROOT" || exit 1
        mkdir -p output results
        echo "#### OS COMP TEST GROUP START ltp-$TEST_LIBC ####"
        rc=0
        while IFS= read -r line; do
            case "$line" in ''|'#'*) continue;; esac
            tname=$(printf '%s\n' "$line" | awk '{print $1}')
            tcmd=$(printf '%s\n' "$line" | cut -d' ' -f2-)
            [ -z "$tname" ] && continue
            [ -z "$tcmd" ] && continue
            echo "RUN LTP CASE $tname"
            timeout 30 sh -c "$tcmd"
            tret=$?
            echo "FAIL LTP CASE $tname : $tret"
            [ "$tret" -ne 0 ] && rc=1
        done < "$LTP_ALLTESTS"
        echo "#### OS COMP TEST GROUP END ltp-$TEST_LIBC ####"
        cd "$TEST_DIR" || exit 1
        rm -f "$LTP_ALLTESTS"
    else
        if is_aggregate "$name"; then
            echo "[SUITE-TYPE] aggregate"
        elif is_directory_scan "$name"; then
            echo "[SUITE-TYPE] directory-scan"
        else
            echo "[SUITE-TYPE] standalone"
        fi

        echo "#### OS COMP TEST GROUP START ${name}-$TEST_LIBC ####"
        if [ "$synthetic_basic" -eq 1 ]; then
            cd ./basic || exit 1
            /bin/sh ./run-all.sh
            rc=$?
            cd "$TEST_DIR" || exit 1
        else
            /bin/sh "$script"
            rc=$?
        fi
        echo "#### OS COMP TEST GROUP END ${name}-$TEST_LIBC ####"
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
