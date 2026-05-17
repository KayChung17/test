# Build Options
export ARCH := riscv64
export LOG := warn
export DWARF := y
export MEMTRACK := n

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
export APP_FEATURES := qemu

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

ifeq ($(COMPETITION), y)
	APP_FEATURES += competition
endif

default: build
all: kernel-rv kernel-la disk.img

ROOTFS_URL = https://github.com/Starry-OS/rootfs/releases/download/20260214
ROOTFS_IMG = rootfs-$(ARCH).img
TEST_IMG ?= test.img
RV_ELF_GLOB = *_riscv64-*.elf
LA_ELF_GLOB = *_loongarch64-*.elf

rootfs:
	@if [ ! -f $(ROOTFS_IMG) ]; then \
		echo "Image not found, downloading..."; \
		curl -f -L $(ROOTFS_URL)/$(ROOTFS_IMG).xz -O; \
		xz -d $(ROOTFS_IMG).xz; \
	fi
	@cp $(ROOTFS_IMG) make/disk.img
	@echo "Rootfs ready: make/disk.img"

aux: $(TEST_IMG) scripts/gen-aux-img.sh
	@scripts/gen-aux-img.sh $(TEST_IMG) make/disk.img 128
	@echo "Auxiliary rootfs ready: make/disk.img"

img:
	@echo -e "\033[33mWARN: The 'img' target is deprecated. Please use 'rootfs' instead.\033[0m"
	@$(MAKE) --no-print-directory rootfs

defconfig justrun clean:
	@$(MAKE) -C make $@

build run debug disasm: defconfig
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

.PHONY: build run justrun debug disasm clean rootfs aux
