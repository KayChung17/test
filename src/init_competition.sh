#!/bin/sh

export HOME=/

TEST_DIR="/oscomp/glibc"

if [ ! -d "$TEST_DIR" ]; then
    echo "=== Starry OS ==="
    echo "Test dir not found: $TEST_DIR"
    cd /root
    exec sh --login
fi

echo "=== Starry OS Competition Mode ==="
echo "Test dir: $TEST_DIR"

PASS=0
FAIL=0
SKIP=0

# ---- basic syscall tests ----
echo "[SUITE-BEGIN] basic"

BASIC_DIR="$TEST_DIR/basic"
BASIC_TESTS="brk chdir clone close dup dup2 execve exit fork fstat getcwd getdents getpid getppid gettimeofday mkdir_ mmap mount munmap open openat pipe read sleep test_echo times umount uname unlink wait waitpid write yield"

for t in $BASIC_TESTS; do
    bin="$BASIC_DIR/$t"
    if [ -x "$bin" ]; then
        echo "[CASE-BEGIN] $t"
        "$bin"
        rc=$?
        if [ "$rc" -eq 0 ]; then
            PASS=$((PASS + 1))
        else
            FAIL=$((FAIL + 1))
        fi
        echo "[CASE-END] $t code=$rc"
    else
        echo "[CASE-SKIP] $t (missing)"
        SKIP=$((SKIP + 1))
    fi
done

echo "[SUITE-END] basic"
echo "PASS: $PASS  FAIL: $FAIL  SKIP: $SKIP"

# ---- glibc dynamic linker setup ----
# LTP binaries need ld-linux-riscv64-lp64d.so.1 at /lib
GLIBC_LIB="$TEST_DIR/lib"
ln -sf "$GLIBC_LIB/ld-linux-riscv64-lp64d.so.1" /lib/
ln -sf "$GLIBC_LIB/libc.so.6" /lib/
ln -sf "$GLIBC_LIB/libm.so.6" /lib/

# ---- LTP fs tests ----
LTP_DIR="$TEST_DIR/ltp/testcases/bin"
echo "[SUITE-BEGIN] ltp-fs"

LTP_FS_TESTS="fchmod01 fchmod02 fchmod03 fchmod04 fchmod05 fchmod06 fchmodat01 fchmodat02"

for t in $LTP_FS_TESTS; do
    bin="$LTP_DIR/$t"
    if [ -x "$bin" ]; then
        echo "[CASE-BEGIN] $t"
        "$bin"
        rc=$?
        if [ "$rc" -eq 0 ]; then
            PASS=$((PASS + 1))
        else
            FAIL=$((FAIL + 1))
        fi
        echo "[CASE-END] $t code=$rc"
    else
        echo "[CASE-SKIP] $t (missing)"
        SKIP=$((SKIP + 1))
    fi
done

echo "[SUITE-END] ltp-fs"
echo "PASS: $PASS  FAIL: $FAIL  SKIP: $SKIP"

# ---- all done ----
echo "=== tests done, powering off ==="
