#!/bin/sh

export HOME=/
TEST_DIR="/oscomp/glibc"
LTPROOT="$TEST_DIR/ltp"

if [ ! -d "$TEST_DIR" ]; then
    echo "=== Starry OS ==="
    echo "Test dir not found: $TEST_DIR"
    cd /root
    exec sh --login
fi

echo "=== Starry OS Competition Focus Mode ==="
echo "Test dir: $TEST_DIR"

GLIBC_LIB="$TEST_DIR/lib"
ln -sf "$GLIBC_LIB/ld-linux-riscv64-lp64d.so.1" /lib/
ln -sf "$GLIBC_LIB/libc.so.6" /lib/
ln -sf "$GLIBC_LIB/libm.so.6" /lib/

cd "$TEST_DIR"

echo "[SUITE-BEGIN] ltp_fs_focus"
echo "#### OS COMP TEST GROUP START ltp-fs-focus-glibc ####"

tmp_cmds="/tmp/ltp_fs_focus_syscalls.$$"
grep -E '^(fcntl12|fcntl13|fcntl14)([[:space:]]|$)' "$LTPROOT/runtest/syscalls" > "$tmp_cmds" || exit 1

cd "$LTPROOT" || exit 1
./runltp -f "$tmp_cmds"
rc=$?
rm -f "$tmp_cmds"

cd "$TEST_DIR" || exit 1

echo "#### OS COMP TEST GROUP END ltp-fs-focus-glibc ####"
echo "[SUITE-RESULT] ltp_fs_focus exit=$rc"
echo "[SUITE-END] ltp_fs_focus"
echo "[SUMMARY] suites=1 pass=$([ "$rc" -eq 0 ] && echo 1 || echo 0) fail=$([ "$rc" -eq 0 ] && echo 0 || echo 1) skip=0"
echo "=== tests done, powering off ==="
