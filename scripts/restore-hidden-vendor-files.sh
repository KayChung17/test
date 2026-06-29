#!/bin/bash
# Restore hidden vendor files that are filtered out by the evaluator when it
# clones the repository (notably `.cargo-checksum.json` in Cargo vendor dirs).

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
VENDOR_DIR="$ROOT/vendor"

if [ ! -d "$VENDOR_DIR" ]; then
    exit 0
fi

restored=0
missing=0

while IFS= read -r -d '' crate_dir; do
    template="$crate_dir/cargo-checksum.json"
    hidden="$crate_dir/.cargo-checksum.json"

    if [ -f "$template" ]; then
        cp "$template" "$hidden"
        restored=$((restored + 1))
        continue
    fi

    if [ ! -f "$hidden" ]; then
        echo "Missing vendored checksum template: ${crate_dir#$ROOT/}" >&2
        missing=1
    fi
done < <(find "$VENDOR_DIR" -mindepth 1 -maxdepth 1 -type d -print0)

if [ "$missing" -ne 0 ]; then
    echo "Vendor hidden-file restoration failed; regenerate checksum templates before building." >&2
    exit 1
fi

if [ "${RESTORE_VENDOR_VERBOSE:-0}" = "1" ]; then
    echo "Restored $restored vendored checksum files"
fi
