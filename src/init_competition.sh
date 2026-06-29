#!/bin/sh

export HOME=/
export KCONFIG_PATH=/proc/config

# Read libc and suite selection from files if present (for local testing).
[ -f /etc/test_libc ] && TEST_LIBCS=$(cat /etc/test_libc)
[ -f /etc/only_suites ] && ONLY_SUITES=$(cat /etc/only_suites)
[ -f /etc/skip_suites ] && SKIP_SUITES=$(cat /etc/skip_suites)
[ -f /etc/ltp_case_timeout ] && LTP_CASE_TIMEOUT=$(cat /etc/ltp_case_timeout)
[ -f /etc/suite_timeout_ltp ] && SUITE_TIMEOUT_LTP=$(cat /etc/suite_timeout_ltp)
[ -f /etc/suite_timeout_default ] && SUITE_TIMEOUT_DEFAULT=$(cat /etc/suite_timeout_default)

TEST_LIBCS="${TEST_LIBCS:-glibc musl}"
SUITE_TIMEOUT_DEFAULT="${SUITE_TIMEOUT_DEFAULT:-600}"

FOUND_TEST_DIR=0
for libc in $TEST_LIBCS; do
    if [ -d "/oscomp/$libc" ]; then
        FOUND_TEST_DIR=1
    fi
done

if [ "$FOUND_TEST_DIR" -eq 0 ]; then
    echo "=== Starry OS ==="
    echo "Test dirs not found under /oscomp for: $TEST_LIBCS"
    cd /root
    exec sh --login
fi

echo "=== Starry OS Competition Mode ==="

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
cat > /etc/protocols <<'EOF'
hopopt 0 HOPOPT
ipv6 41 IPv6
ipv6-route 43 IPv6-Route
ipv6-frag 44 IPv6-Frag
esp 50 ESP
ah 51 AH
ipv6-icmp 58 IPv6-ICMP
ipv6-nonxt 59 IPv6-NoNxt
ipv6-opts 60 IPv6-Opts
tcp 6 TCP
udp 17 UDP
icmp 1 ICMP
EOF

# ---- dynamic linker setup ----
LIBC_LIB="$TEST_DIR/lib"
GLIBC_LIB="/oscomp/glibc/lib"
mkdir -p /lib /lib64

link_glibc_runtime() {
    [ -d "$GLIBC_LIB" ] || return 0
    for loader in \
        ld-linux-riscv64-lp64d.so.1 \
        ld-linux-loongarch-lp64d.so.1
    do
        if [ -f "$GLIBC_LIB/$loader" ]; then
            ln -sf "$GLIBC_LIB/$loader" "/lib/$loader"
            ln -sf "$GLIBC_LIB/$loader" "/lib64/$loader"
        fi
    done
    for lib in libc.so.6 libm.so.6; do
        if [ -f "$GLIBC_LIB/$lib" ]; then
            ln -sf "$GLIBC_LIB/$lib" "/lib/$lib"
            ln -sf "$GLIBC_LIB/$lib" "/lib64/$lib"
        fi
    done
}

# Some payload directories mix libc families: for example, several musl/basic
# binaries still request the glibc interpreter. Keep both runtimes available.
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

run_ltp_all_cases() {
    local suite_timeout="$1"
    local target_dir="ltp/testcases/bin"
    local case_timeout="${LTP_CASE_TIMEOUT:-30}"
    local start_time now elapsed remaining current_timeout
    local file base ret pid watchdog

    start_time=$(date +%s 2>/dev/null || echo 0)

    echo "#### OS COMP TEST GROUP START ltp-$TEST_LIBC ####"
    echo "[LTP] target_dir=$target_dir case_timeout=${case_timeout}s suite_timeout=${suite_timeout:-0}s"

    for file in "$target_dir"/*; do
        [ -f "$file" ] || continue
        current_timeout="$case_timeout"
        if [ -n "$suite_timeout" ] && [ "$suite_timeout" != "0" ] && [ "$start_time" != "0" ]; then
            now=$(date +%s 2>/dev/null || echo "$start_time")
            elapsed=$((now - start_time))
            if [ "$elapsed" -ge "$suite_timeout" ]; then
                echo "[LTP-SUITE-TIMEOUT] ltp-$TEST_LIBC after ${suite_timeout}s"
                echo "#### OS COMP TEST GROUP END ltp-$TEST_LIBC ####"
                return 124
            fi
            remaining=$((suite_timeout - elapsed))
            if [ "$remaining" -lt "$current_timeout" ]; then
                current_timeout="$remaining"
            fi
            [ "$current_timeout" -gt 0 ] || current_timeout=1
        fi
        base="${file##*/}"
        echo "RUN LTP CASE $base"

        if command -v setsid >/dev/null 2>&1; then
            setsid "$file" &
        else
            "$file" &
        fi
        pid=$!
        (
            sleep "$current_timeout"
            kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || exit 0
            sleep 1
            kill -KILL "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
        ) &
        watchdog=$!

        wait "$pid"
        ret=$?
        kill "$watchdog" 2>/dev/null || true
        wait "$watchdog" 2>/dev/null || true

        if [ "$ret" -eq 124 ] || [ "$ret" -eq 137 ] || [ "$ret" -eq 143 ]; then
            echo "[LTP-CASE-TIMEOUT] $base after ${current_timeout}s"
        fi
        echo "FAIL LTP CASE $base : $ret"
    done

    echo "#### OS COMP TEST GROUP END ltp-$TEST_LIBC ####"
    return 0
}

suite_timeout_for() {
    name="$1"

    case "$name" in
        basic)
            echo "${SUITE_TIMEOUT_BASIC:-0}"
            ;;
        ltp)
            echo "${SUITE_TIMEOUT_LTP:-900}"
            ;;
        *)
            echo "${SUITE_TIMEOUT_DEFAULT:-0}"
            ;;
    esac
}

run_script_with_timeout() {
    script="$1"
    log_file="$2"
    timeout_seconds="$3"

    RUN_RC=0
    if [ -z "$timeout_seconds" ] || [ "$timeout_seconds" = "0" ]; then
        /bin/sh "$script" >"$log_file" 2>&1
        RUN_RC=$?
        return 0
    fi

    rc_file=$(mktemp "/tmp/suite-rc.XXXXXX") || exit 1
    (
        /bin/sh "$script" >"$log_file" 2>&1
        echo "$?" >"$rc_file"
    ) &
    suite_pid=$!
    (
        sleep "$timeout_seconds"
        kill -TERM "$suite_pid" 2>/dev/null || exit 0
        sleep 1
        kill -KILL "$suite_pid" 2>/dev/null || true
    ) &
    watchdog_pid=$!

    wait "$suite_pid"
    wait_rc=$?
    kill "$watchdog_pid" 2>/dev/null || true
    wait "$watchdog_pid" 2>/dev/null || true

    if [ -s "$rc_file" ]; then
        RUN_RC=$(cat "$rc_file")
    else
        case "$wait_rc" in
            124|137|143)
                RUN_RC=$wait_rc
                ;;
            *)
                RUN_RC=$wait_rc
                ;;
        esac
    fi
    rm -f "$rc_file"
    return 0
}

run_ltp_with_timeout() {
    timeout_seconds="$1"

    RUN_RC=0
    run_ltp_all_cases "${timeout_seconds:-0}" 2>&1
    RUN_RC=$?
    return 0
}

suite_selected() {
    name="$1"

    if [ -n "$ONLY_SUITES" ]; then
        case " $ONLY_SUITES " in
            *" $name "*) ;;
            *) return 1 ;;
        esac
    fi
    if [ -n "$SKIP_SUITES" ]; then
        case " $SKIP_SUITES " in
            *" $name "*) return 1 ;;
        esac
    fi
    return 0
}

run_libc_suites() {
    TEST_LIBC="$1"
    TEST_DIR="/oscomp/$TEST_LIBC"
    LTPROOT="$TEST_DIR/ltp"
    export TEST_LIBC TEST_DIR LTPROOT

    if [ ! -d "$TEST_DIR" ]; then
        echo "[SUMMARY] skip $TEST_LIBC: test dir not found: $TEST_DIR"
        return 0
    fi

    echo "Test dir: $TEST_DIR"

    LIBC_LIB="$TEST_DIR/lib"
    GLIBC_LIB="/oscomp/glibc/lib"
    link_glibc_runtime

    if [ "$TEST_LIBC" = "glibc" ]; then
        export LD_LIBRARY_PATH="$LIBC_LIB:/lib:/lib64"
    else
        export LD_LIBRARY_PATH="$LIBC_LIB:$GLIBC_LIB:/lib:/lib64"
        for loader in \
            ld-musl-riscv64.so.1 \
            ld-musl-riscv64-sf.so.1 \
            ld-musl-loongarch-lp64d.so.1
        do
            if [ -f "$LIBC_LIB/$loader" ]; then
                loader_target="$LIBC_LIB/$loader"
            elif [ -f "$LIBC_LIB/libc.so" ]; then
                loader_target="$LIBC_LIB/libc.so"
            else
                continue
            fi
            ln -sf "$loader_target" "/lib/$loader"
            ln -sf "$loader_target" "/lib64/$loader"
        done
        [ -f "$LIBC_LIB/libc.so" ] && ln -sf "$LIBC_LIB/libc.so" /lib/libc.so
    fi
    for lib in "$LIBC_LIB"/*.so*; do
        [ -f "$lib" ] || continue
        base="${lib##*/}"
        ln -sf "$lib" "/lib/$base"
        ln -sf "$lib" "/lib64/$base"
    done

    # The rv test image keeps lmbench wrappers built with an absolute path under
    # /code/lmbench_src/bin/build. Mirror that path to the mounted test payload.
    if [ -x "$TEST_DIR/lmbench_all" ]; then
        mkdir -p /code/lmbench_src/bin/build
        ln -sf "$TEST_DIR/lmbench_all" /code/lmbench_src/bin/build/lmbench_all
    fi

    cd "$TEST_DIR" || return 1

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
        return 0
    fi

    SUITE_PASS=0
    SUITE_FAIL=0

    for script in $SCRIPTS; do
        name="${script%_testcode.sh}"
        suite_selected "$name" || continue

        echo "[SUITE-BEGIN] $name-$TEST_LIBC"

        # recognize bench type
        if is_aggregate "$name"; then
            echo "[SUITE-TYPE] aggregate"
        elif is_directory_scan "$name"; then
            echo "[SUITE-TYPE] directory-scan"
        else
            echo "[SUITE-TYPE] standalone"
        fi

        if [ "$name" = "lmbench" ]; then
            export ENOUGH="${ENOUGH:-50000}"
        fi
        if [ "$name" = "ltp" ]; then
            suite_timeout=$(suite_timeout_for "$name")
            run_ltp_with_timeout "$suite_timeout"
            rc=$RUN_RC
            if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ] || [ "$rc" -eq 143 ]; then
                echo "[SUITE-TIMEOUT] $name-$TEST_LIBC after ${suite_timeout}s"
            fi
        else
            suite_timeout=$(suite_timeout_for "$name")
            suite_log=$(mktemp "/tmp/${name}.XXXXXX") || exit 1
            run_script_with_timeout "$script" "$suite_log" "$suite_timeout"
            rc=$RUN_RC
            sed \
                -e "s/^#### OS COMP TEST GROUP START ${name} ####$/#### OS COMP TEST GROUP START ${name}-$TEST_LIBC ####/" \
                -e "s/^#### OS COMP TEST GROUP END ${name} ####$/#### OS COMP TEST GROUP END ${name}-$TEST_LIBC ####/" \
                "$suite_log"
            rm -f "$suite_log"
            if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ] || [ "$rc" -eq 143 ]; then
                echo "[SUITE-TIMEOUT] $name-$TEST_LIBC after ${suite_timeout}s"
            fi
        fi

        echo "[SUITE-RESULT] $name-$TEST_LIBC exit=$rc"
        if [ "$rc" -eq 0 ]; then
            SUITE_PASS=$((SUITE_PASS + 1))
        else
            SUITE_FAIL=$((SUITE_FAIL + 1))
        fi
        echo "[SUITE-END] $name-$TEST_LIBC"
    done

    echo "[SUMMARY] $TEST_LIBC suites=$((SUITE_PASS + SUITE_FAIL)) pass=$SUITE_PASS fail=$SUITE_FAIL skip=0"
    TOTAL_PASS=$((TOTAL_PASS + SUITE_PASS))
    TOTAL_FAIL=$((TOTAL_FAIL + SUITE_FAIL))
}

TOTAL_PASS=0
TOTAL_FAIL=0
for libc in $TEST_LIBCS; do
    run_libc_suites "$libc"
done

echo "[SUMMARY] all suites=$((TOTAL_PASS + TOTAL_FAIL)) pass=$TOTAL_PASS fail=$TOTAL_FAIL skip=0"
poweroff_after_tests
