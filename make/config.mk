# Config generation

FULL_CONFIG := $(if $(filter $(ARCH),riscv64),$(abspath $(CURDIR)/../configs/riscv64-qemu-virt.axconfig.toml),$(if $(filter $(ARCH),loongarch64),$(abspath $(CURDIR)/../configs/loongarch64-qemu-virt.axconfig.toml),))

ifneq ($(MEM),)
  MEM_BYTES := $(shell ./strtosz.py $(MEM))
else
  MEM_BYTES := $(shell sed -n 's/^phys-memory-size = \(.*\) # uint/\1/p' $(FULL_CONFIG) | head -1 | tr -d _)
  MEM := $(shell printf "%dB" $(MEM_BYTES))
endif

ifneq ($(SMP),)
  SMP_VALUE := $(SMP)
else
  SMP_VALUE := $(shell sed -n 's/^max-cpu-num = \(.*\) # uint/\1/p' $(FULL_CONFIG) | head -1)
  SMP := $(SMP_VALUE)
  ifeq ($(SMP),)
    $(error "`plat.max-cpu-num` is not defined in the platform configuration file, this option must be specified even for platforms with runtime CPU detection.")
  endif
endif

define defconfig
  @cp $(FULL_CONFIG) "$(OUT_CONFIG)"
endef

define oldconfig
  $(call defconfig)
endef
