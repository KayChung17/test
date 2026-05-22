# Necessary dependencies for the build system

# Offline submission builds must not download tools on demand.
ifeq ($(shell llvm-objcopy --version 2>/dev/null),)
  $(error Missing required host tool: llvm-objcopy)
endif

ifeq ($(DWARF), y)
  ifeq ($(shell llvm-objdump --version 2>/dev/null),)
    $(error Missing required host tool: llvm-objdump)
  endif
endif
