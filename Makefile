# Build Options
export ARCH := riscv64
export LOG := warn
export DWARF := y
export MEMTRACK := n
export CARGO_TARGET_DIR := $(PWD)/target
export CARGO_NET_OFFLINE := true
export CARGO_TERM_COLOR := always
export RUSTUP_OFFLINE := true
export RUSTUP_TOOLCHAIN := nightly-2025-05-20-x86_64-unknown-linux-gnu
export RUSTC_HOST := $(shell rustup run $(RUSTUP_TOOLCHAIN) rustc -vV 2>/dev/null | sed -n 's/^host: //p')
export RUST_SYSROOT := $(shell rustup run $(RUSTUP_TOOLCHAIN) rustc --print sysroot 2>/dev/null)
export RUST_LLVM_TOOLS_DIR := $(RUST_SYSROOT)/lib/rustlib/$(RUSTC_HOST)/bin

ifneq ($(wildcard $(RUST_LLVM_TOOLS_DIR)/llvm-objcopy),)
export PATH := $(RUST_LLVM_TOOLS_DIR):$(PATH)
endif

# QEMU Options
export BLK := y
export NET := y
export VSOCK := n
export MEM := 1G
export ICOUNT := n

# Generated Options
export A := $(PWD)
export NO_AXSTD := y
export AX_LIB := axfeat
export COMPETITION ?= n
export APP_FEATURES := $(if $(filter $(ARCH),loongarch64),qemu-pci,qemu)

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

ifeq ($(COMPETITION), y)
	APP_FEATURES += competition
endif

default: build
all: kernel-rv kernel-la disk.img

ROOTFS_IMG = rootfs-$(ARCH).img
ROOTFS_SIZE_MB ?= 128
ROOTFS_SOURCE_DIR ?= rootfs-source/$(ARCH)
ROOTFS_SOURCE_IMG ?=
TEST_IMG ?= test.img
ROOTFS_TEST_IMG_CANDIDATES := $(if $(ROOTFS_SOURCE_IMG),$(ROOTFS_SOURCE_IMG)) $(if $(TEST_IMG),$(TEST_IMG)) make/test.img test.img $(if $(filter $(ARCH),riscv64),tmp/disk-rv.img tmp/disk.img sdcard-rv.img,$(if $(filter $(ARCH),loongarch64),tmp/disk-la.img tmp/disk.img sdcard-la.img))
RV_ELF_GLOB = *_riscv64-*.elf
LA_ELF_GLOB = *_loongarch64-*.elf

prepare-cargo-home:
	@mkdir -p .cargo
	@cp cargo-config.toml .cargo/config.toml
	@chmod +x scripts/restore-hidden-vendor-files.sh
	@./scripts/restore-hidden-vendor-files.sh

rootfs:
	@set -e; \
	source_img=""; \
	if [ -f make/disk.img ]; then \
		echo "Rootfs ready: make/disk.img"; \
	elif [ -f disk.img ]; then \
		cp disk.img make/disk.img; \
		echo "Rootfs ready: make/disk.img"; \
	elif [ -f $(ROOTFS_IMG) ]; then \
		cp $(ROOTFS_IMG) make/disk.img; \
		echo "Rootfs ready: make/disk.img"; \
	elif [ -f $(ROOTFS_SOURCE_DIR)/bin/busybox ] && ls $(ROOTFS_SOURCE_DIR)/lib/ld-musl-*.so.1 >/dev/null 2>&1 && [ -f $(ROOTFS_SOURCE_DIR)/etc/passwd ] && [ -f $(ROOTFS_SOURCE_DIR)/etc/group ]; then \
		echo "Generating auxiliary rootfs from $(ROOTFS_SOURCE_DIR) ..."; \
		scripts/gen-aux-img.sh --from-dir "$(ROOTFS_SOURCE_DIR)" make/disk.img $(ROOTFS_SIZE_MB); \
		echo "Auxiliary rootfs ready: make/disk.img"; \
	else \
		for candidate in $(ROOTFS_TEST_IMG_CANDIDATES); do \
			if [ -n "$$candidate" ] && [ -f "$$candidate" ]; then \
				source_img="$$candidate"; \
				break; \
			fi; \
		done; \
		if [ -n "$$source_img" ]; then \
			echo "Generating auxiliary rootfs from $$source_img ..."; \
			scripts/gen-aux-img.sh "$$source_img" make/disk.img $(ROOTFS_SIZE_MB); \
			echo "Auxiliary rootfs ready: make/disk.img"; \
		else \
			echo "Missing rootfs input. Provide disk.img, make/disk.img, $(ROOTFS_IMG), a committed rootfs source under $(ROOTFS_SOURCE_DIR), or a source image via ROOTFS_SOURCE_IMG/TEST_IMG."; \
			exit 1; \
		fi; \
	fi

aux:
	@if [ -d "$(ROOTFS_SOURCE_DIR)" ]; then \
		$(MAKE) --no-print-directory rootfs ROOTFS_SOURCE_DIR="$(ROOTFS_SOURCE_DIR)"; \
	else \
		$(MAKE) --no-print-directory rootfs ROOTFS_SOURCE_IMG=$(if $(ROOTFS_SOURCE_IMG),$(ROOTFS_SOURCE_IMG),$(TEST_IMG)); \
	fi

img:
	@echo -e "\033[33mWARN: The 'img' target is deprecated. Please use 'rootfs' instead.\033[0m"
	@$(MAKE) --no-print-directory rootfs

defconfig justrun clean:
	@$(MAKE) -C make $@

build run debug disasm: prepare-cargo-home defconfig
	@$(MAKE) -C make $@ \
		$(if $(TEST_IMG),TEST_IMG=$(abspath $(TEST_IMG))) \
		DISK_IMG=$(abspath make/disk.img)

kernel-rv: defconfig
	@$(MAKE) ARCH=riscv64 COMPETITION=y build LD_SCRIPT=$(abspath configs/linker_riscv64-qemu-virt_eval.lds)
	@latest=$$(ls -t ./*_riscv64-*.elf 2>/dev/null | head -n 1); \
	if [ -z "$$latest" ]; then \
		echo "No RISC-V ELF artifact found"; \
		exit 1; \
	fi; \
	cp "$$latest" $@

kernel-la: defconfig
	@$(MAKE) ARCH=loongarch64 COMPETITION=y build
	@latest=$$(ls -t ./*_loongarch64-*.elf 2>/dev/null | head -n 1); \
	if [ -z "$$latest" ]; then \
		echo "No LoongArch ELF artifact found"; \
		exit 1; \
	fi; \
	cp "$$latest" $@

disk.img: make/disk.img
	@cp make/disk.img $@

make/disk.img:
	@$(MAKE) rootfs

ci-test:
	./scripts/ci-test.py $(ARCH)

# Aliases
rv:
	$(MAKE) ARCH=riscv64 run

la:
	$(MAKE) ARCH=loongarch64 run

vf2:
	$(MAKE) ARCH=riscv64 APP_FEATURES=vf2 MYPLAT=axplat-riscv64-visionfive2 BUS=mmio build

.PHONY: prepare-cargo-home build run justrun debug disasm clean rootfs aux
