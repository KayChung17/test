# Necessary dependencies for the build system

# Offline submission builds must not download tools on demand.
LLVM_OBJCOPY ?= $(if $(wildcard $(RUST_LLVM_TOOLS_DIR)/llvm-objcopy),$(RUST_LLVM_TOOLS_DIR)/llvm-objcopy,llvm-objcopy)
LLVM_OBJDUMP ?= $(if $(wildcard $(RUST_LLVM_TOOLS_DIR)/llvm-objdump),$(RUST_LLVM_TOOLS_DIR)/llvm-objdump,llvm-objdump)

ifeq ($(shell $(LLVM_OBJCOPY) --version 2>/dev/null),)
  $(error Missing required host tool: llvm-objcopy)
endif

ifeq ($(DWARF), y)
  ifeq ($(shell $(LLVM_OBJDUMP) --version 2>/dev/null),)
    $(error Missing required host tool: llvm-objdump)
  endif
endif
