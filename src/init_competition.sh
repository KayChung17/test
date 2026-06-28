#!/bin/sh

export HOME=/

# Read libc selection from files if present (for local testing)
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

poweroff_after_tests() {
    echo "=== tests done, powering off ==="
    sync 2>/dev/null || true
    if command -v poweroff >/dev/null 2>&1; then
        poweroff -f >/dev/null 2>&1 || true
    fi
    if [ -x /bin/busybox ]; then
        /bin/busybox poweroff -f >/dev/null 2>&1 || true
    fi
    exit 0
}

mkdir -p /etc
if [ ! -f /etc/passwd ]; then
    echo "root:x:0:0:root:/root:/bin/sh" > /etc/passwd
fi
if ! grep -q '^nobody:' /etc/passwd 2>/dev/null; then
    echo "nobody:x:65534:65534:nobody:/:" >> /etc/passwd
fi
if [ ! -f /etc/group ]; then
    echo "root:x:0:" > /etc/group
fi
if ! grep -q '^nobody:' /etc/group 2>/dev/null; then
    echo "nobody:x:65534:" >> /etc/group
fi

# ---- dynamic linker setup ----
LIBC_LIB="$TEST_DIR/lib"
if [ "$TEST_LIBC" = "glibc" ]; then
    export LD_LIBRARY_PATH="$LIBC_LIB:/lib:/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    mkdir -p /lib /lib64
    for loader in \
        ld-linux-riscv64-lp64d.so.1 \
        ld-linux-loongarch-lp64d.so.1
    do
        if [ -f "$LIBC_LIB/$loader" ]; then
            ln -sf "$LIBC_LIB/$loader" "/lib/$loader"
            ln -sf "$LIBC_LIB/$loader" "/lib64/$loader"
        fi
    done
    if [ -f "$LIBC_LIB/libc.so.6" ]; then
        ln -sf "$LIBC_LIB/libc.so.6" /lib/
        ln -sf "$LIBC_LIB/libc.so.6" /lib64/
    fi
    if [ -f "$LIBC_LIB/libm.so.6" ]; then
        ln -sf "$LIBC_LIB/libm.so.6" /lib/
        ln -sf "$LIBC_LIB/libm.so.6" /lib64/
    fi
else
    mkdir -p /lib /lib64
    for loader in \
        ld-musl-riscv64.so.1 \
        ld-musl-loongarch-lp64d.so.1
    do
        if [ -f "$LIBC_LIB/$loader" ]; then
            loader_target="$LIBC_LIB/$loader"
        elif [ -f "$LIBC_LIB/libc.so" ]; then
            loader_target="$LIBC_LIB/libc.so"
        else
            continue
        fi
        [ -e "/lib/$loader" ] || ln -sf "$loader_target" "/lib/$loader"
        [ -e "/lib64/$loader" ] || ln -sf "$loader_target" "/lib64/$loader"
    done
    ln -sf "$LIBC_LIB/libc.so" /lib/libc.so
fi
for lib in "$LIBC_LIB"/*.so*; do
    [ -f "$lib" ] || continue
    base="${lib##*/}"
    ln -sf "$lib" "/lib/$base"
    ln -sf "$lib" "/lib64/$base"
done

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
    poweroff_after_tests
fi

SUITE_PASS=0
SUITE_FAIL=0

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
    echo "[SUITE-BEGIN] $name"

    # recognize bench type
    if is_aggregate "$name"; then
        echo "[SUITE-TYPE] aggregate"
    elif is_directory_scan "$name"; then
        echo "[SUITE-TYPE] directory-scan"
    else
        echo "[SUITE-TYPE] standalone"
    fi

    suite_status=$(mktemp "/tmp/${name}.status.XXXXXX") || exit 1
    rm -f "$suite_status"
    if [ "$name" = "lmbench" ]; then
        export ENOUGH="${ENOUGH:-50000}"
    fi
    (
        /bin/sh "$script" 2>&1
        echo "$?" > "$suite_status"
    ) | sed \
        -e "s/^#### OS COMP TEST GROUP START ${name} ####$/#### OS COMP TEST GROUP START ${name}-$TEST_LIBC ####/" \
        -e "s/^#### OS COMP TEST GROUP END ${name} ####$/#### OS COMP TEST GROUP END ${name}-$TEST_LIBC ####/"
    rc=$(cat "$suite_status" 2>/dev/null || echo 1)
    rm -f "$suite_status"

    echo "[SUITE-RESULT] $name exit=$rc"
    if [ "$rc" -eq 0 ]; then
        SUITE_PASS=$((SUITE_PASS + 1))
    else
        SUITE_FAIL=$((SUITE_FAIL + 1))
    fi
    echo "[SUITE-END] $name"
done

echo "[SUMMARY] suites=$((SUITE_PASS + SUITE_FAIL)) pass=$SUITE_PASS fail=$SUITE_FAIL skip=0"
poweroff_after_tests
