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
cleanup() {
    rm -rf "$PREP_DIR"
}
trap cleanup EXIT

rm -rf "$PREP_DIR"
mkdir -p "$PREP_DIR"
cp -a "$TESTDATA_DIR/." "$PREP_DIR/"
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
    -v "$ROOT:/coursegrader/submit"
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
fi
