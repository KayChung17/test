#!/bin/bash
# Generate minimal auxiliary rootfs image from a source ext4 image.
# Usage: ./scripts/gen-aux-img.sh <source_img> <output_img> [size_mb]
#
# Extracts RISC-V busybox + musl libc + /etc from the source and
# creates a bootable ext4 auxiliary rootfs.

set -e

SRC="${1:?source image required}"
DST="${2:?output image required}"
SIZE="${3:-128}"

SRC=$(realpath "$SRC")
DST=$(realpath -m "$DST")
D=$(dirname "$DST")
mkdir -p "$D"

TMPDIR=$(mktemp -d -t auxbuild.XXXXXX)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Extracting files from $SRC ..."
debugfs -R "dump /bin/busybox              $TMPDIR/busybox"              "$SRC"
debugfs -R "dump /lib/ld-musl-riscv64.so.1 $TMPDIR/ld-musl-riscv64.so.1" "$SRC"
for f in passwd group hostname inittab fstab inputrc; do
    debugfs -R "dump /etc/$f $TMPDIR/$f" "$SRC" 2>/dev/null || true
done

echo "Creating empty ext4 image ($SIZE MB)..."
truncate -s "${SIZE}M" "$DST"
mkfs.ext4 -q -O ^64bit "$DST"

echo "Populating $DST ..."

cd "$TMPDIR"
CMD="$TMPDIR/cmds"

# Build the debugfs command script dynamically, adding optional /etc files
# only if they were extracted successfully.
cat > "$CMD" <<'HEADER'
mkdir /bin
mkdir /dev
mkdir /etc
mkdir /home
mkdir /lib
mkdir /media
mkdir /mnt
mkdir /opt
mkdir /proc
mkdir /root
mkdir /run
mkdir /sbin
mkdir /srv
mkdir /sys
mkdir /tmp
mkdir /usr
mkdir /var
mkdir /oscomp

cd /bin
write busybox busybox
set_inode_field busybox mode 0100755
ln busybox sh
ln busybox env
ln busybox echo
ln busybox cat
ln busybox ls
ln busybox mount
ln busybox umount
ln busybox mkdir
ln busybox grep
ln busybox sed
ln busybox awk
ln busybox ps
ln busybox kill
ln busybox sleep
ln busybox test
ln busybox [
ln busybox chmod
ln busybox cp
ln busybox mv
ln busybox rm
ln busybox dd
ln busybox df
ln busybox dmesg
ln busybox sync
ln busybox tar
ln busybox gzip
ln busybox gunzip
ln busybox vi
ln busybox clear
ln busybox printf
ln busybox wget
ln busybox uname
ln busybox id
ln busybox whoami

cd /lib
write ld-musl-riscv64.so.1 ld-musl-riscv64.so.1
set_inode_field ld-musl-riscv64.so.1 mode 0100755
ln ld-musl-riscv64.so.1 libc.musl-riscv64.so.1

cd /etc
write passwd passwd
write group group
HEADER

for f in hostname inittab fstab inputrc; do
    [ -f "$f" ] && echo "write $f $f"
done >> "$CMD"

debugfs -w "$DST" < "$CMD"

echo "Done: $DST ($SIZE MB)"
ls -lh "$DST"
