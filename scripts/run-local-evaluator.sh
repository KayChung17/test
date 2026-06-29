#!/bin/bash
# Reproduce the autotest evaluator locally inside Docker.
#
# Usage:
#   ./scripts/run-local-evaluator.sh [full|build-only]
#
# Modes:
#   full       Run autotest prework -> run -> postwork (default)
#   build-only Run only the evaluator compile step inside Docker
#
# Environment variables:
#   AUTOTEST_REPO   Path to autotest-for-oskernel repo
#   TESTDATA_DIR    Path to local evaluator testdata directory
#   DOCKER_IMAGE    Evaluator image tag
#   RV_IMG          Raw RISC-V evaluator image (.img) to gzip if needed
#   LA_IMG          Raw LoongArch evaluator image (.img) to gzip if needed
#   HOOK_DIR        Writable host directory for /mnt/cghook
#
# Notes:
# - The evaluator code expects /coursegrader/testdata/sdcard-rv.img.gz and
#   /coursegrader/testdata/sdcard-la.img.gz. This script prepares those in a
#   temporary testdata directory from raw .img files when necessary.
# - The submission repo is mounted at /coursegrader/submit and built via
#   `make all`, matching evaluator behavior.

set -euo pipefail

MODE="${1:-full}"
case "$MODE" in
    full|build-only) ;;
    *)
        echo "Usage: $0 [full|build-only]" >&2
        exit 2
        ;;
esac

ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
AUTOTEST_REPO=${AUTOTEST_REPO:-$(realpath -m "$ROOT/../autotest-for-oskernel")}
TESTDATA_DIR=${TESTDATA_DIR:-$(realpath -m "$ROOT/../oskernel-autotest-data")}
DOCKER_IMAGE=${DOCKER_IMAGE:-zhouzhouyi/os-contest:20260510}
HOOK_DIR=${HOOK_DIR:-$TESTDATA_DIR}

if [ ! -d "$AUTOTEST_REPO/kernel" ]; then
    echo "Missing autotest repo: $AUTOTEST_REPO" >&2
    exit 1
fi

if [ ! -d "$TESTDATA_DIR" ]; then
    echo "Missing testdata dir: $TESTDATA_DIR" >&2
    exit 1
fi

find_default_img() {
    local var_name="$1"
    shift
    local candidate
    for candidate in "$@"; do
        if [ -n "$candidate" ] && [ -f "$candidate" ]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

RV_IMG=${RV_IMG:-}
LA_IMG=${LA_IMG:-}

if [ -z "$RV_IMG" ]; then
    RV_IMG=$(find_default_img RV_IMG \
        "$ROOT/sdcard-rv.img" \
        "$ROOT/tmp/sdcard-rv.img" \
        "$ROOT/../testsuits-for-oskernel/sdcard-rv.img" \
        2>/dev/null || true)
fi

if [ -z "$LA_IMG" ]; then
    LA_IMG=$(find_default_img LA_IMG \
        "$ROOT/sdcard-la.img" \
        "$ROOT/tmp/sdcard-la.img" \
        "$ROOT/../testsuits-for-oskernel/sdcard-la.img" \
        2>/dev/null || true)
fi

PREP_DIR="$ROOT/tmp/local-evaluator-data"
PREP_SUBMIT_DIR="$ROOT/tmp/local-evaluator-submit"
cleanup() {
    rm -rf "$PREP_DIR" "$PREP_SUBMIT_DIR"
}
trap cleanup EXIT

rm -rf "$PREP_DIR" "$PREP_SUBMIT_DIR"
mkdir -p "$PREP_DIR" "$PREP_SUBMIT_DIR"
cp -a "$TESTDATA_DIR/." "$PREP_DIR/"

rsync -a --delete \
    --exclude='.git/' \
    --exclude='.claude/' \
    --exclude='target/' \
    --exclude='tmp/' \
    --exclude='disk.img' \
    --exclude='kernel-rv' \
    --exclude='kernel-la' \
    --exclude='sdcard-rv.img' \
    --exclude='sdcard-la.img' \
    --exclude='disk-rv.img' \
    --exclude='disk-la.img' \
    --exclude='os_serial_out_rv.txt' \
    --exclude='os_serial_out_la.txt' \
    --exclude='submit_riscv64-qemu-virt.bin' \
    --exclude='submit_riscv64-qemu-virt.elf' \
    --exclude='submit_loongarch64-qemu-virt.bin' \
    --exclude='submit_loongarch64-qemu-virt.elf' \
    "$ROOT/" "$PREP_SUBMIT_DIR/"
mkdir -p "$HOOK_DIR"
mkdir -p "$PREP_DIR/cghook"

prepare_gz() {
    local raw_img="$1"
    local gz_path="$2"
    if [ -f "$gz_path" ] && [ -s "$gz_path" ]; then
        return 0
    fi
    if [ -z "$raw_img" ] || [ ! -f "$raw_img" ]; then
        return 1
    fi
    rm -f "$gz_path"
    gzip -c "$raw_img" > "$gz_path"
    [ -s "$gz_path" ]
}

if [ "$MODE" = full ]; then
    if ! prepare_gz "$RV_IMG" "$PREP_DIR/sdcard-rv.img.gz"; then
        echo "Missing RISC-V evaluator image. Set RV_IMG or provide sdcard-rv.img(.gz)." >&2
        exit 1
    fi
    if ! prepare_gz "$LA_IMG" "$PREP_DIR/sdcard-la.img.gz"; then
        echo "Missing LoongArch evaluator image. Set LA_IMG or provide sdcard-la.img(.gz)." >&2
        exit 1
    fi
fi

DOCKER_BASE=(
    docker run --rm -i
    -v "$PREP_SUBMIT_DIR:/coursegrader/submit"
    -v "$PREP_DIR:/coursegrader/testdata"
    -v "$AUTOTEST_REPO:/home/cguser"
    -v "$HOOK_DIR:/mnt/cghook"
    -w /home/cguser/kernel
    -u root
    "$DOCKER_IMAGE"
)

if [ "$MODE" = build-only ]; then
    echo "[local-evaluator] build-only mode"
    "${DOCKER_BASE[@]}" python3 - <<'PY'
from pygrading import Job
from prework import prework
from utils import Env

env = Env()
env.load_config()
job = Job(config=env.config)
prework(job)
print("PREWORK_OK")
PY
else
    echo "[local-evaluator] full mode"
    echo "[local-evaluator] using RV image: ${RV_IMG:-<from existing .gz>}"
    echo "[local-evaluator] using LA image: ${LA_IMG:-<from existing .gz>}"
    ls -lh "$PREP_DIR"/sdcard-rv.img.gz "$PREP_DIR"/sdcard-la.img.gz
    "${DOCKER_BASE[@]}" python3 .
    echo "[local-evaluator] recomputing suite summary"
    python3 - <<PY
from pathlib import Path
import sys
sys.path.insert(0, str(Path(r"$AUTOTEST_REPO") / "kernel"))
from run import parse_serial_out_new
from postwork import build_table

config = {'testcase_dir': r"$TESTDATA_DIR"}
rv_log = Path(r"$PREP_SUBMIT_DIR") / 'os_serial_out_rv.txt'
la_log = Path(r"$PREP_SUBMIT_DIR") / 'os_serial_out_la.txt'
rv = parse_serial_out_new(config, str(rv_log))
la = parse_serial_out_new(config, str(la_log))
summary = {'rv': rv, 'la': la}
wanted = ['basic-glibc','busybox-glibc','cyclictest-glibc','iozone-glibc','iperf-glibc','libcbench-glibc','libctest-glibc','lmbench-glibc','ltp-glibc','lua-glibc','netperf-glibc','unixbench-glibc']
print('LOCAL_SUITE_SUMMARY')
total = 0.0
for g in wanted:
    s, _ = build_table(g, ['rv','la'], summary)
    rv_score = s.get('rv', 0)
    la_score = s.get('la', 0)
    suite_total = s.get('#TOTAL', 0)
    total += suite_total
    point = g.split('-', 1)[0]
    print(f'{point}\trv={rv_score}\tla={la_score}\ttotal={suite_total}')
print(f'LOCAL_TOTAL\t{total}')
PY
fi
