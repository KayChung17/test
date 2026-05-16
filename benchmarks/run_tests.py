#!/usr/bin/env python3
"""Run benchmark tests in QEMU and collect results using pexpect."""
import os
import sys
import pexpect

PROJECT_DIR = "/home/tianaoli/os/oskernel2026-minux"
PROMPT = "starry:~#"
RESULTS = {}

os.environ["PATH"] = (
    "/home/tianaoli/.cargo/bin:"
    "/home/tianaoli/.rustup/toolchains/nightly-2026-02-25-x86_64-unknown-linux-gnu/bin:"
    + os.environ.get("PATH", "")
)

# Ensure defconfig is done first
print("Running defconfig...", file=sys.stderr)
os.system(f"make -C {PROJECT_DIR} defconfig 2>/dev/null")

print("Starting QEMU...", file=sys.stderr)
cmd = (
    f"make -C {PROJECT_DIR} ARCH=riscv64 ACCEL=n justrun "
    'QEMU_ARGS="-monitor none -nographic"'
)

child = pexpect.spawn(cmd, encoding='utf-8', timeout=60, codec_errors='replace')
child.logfile = sys.stderr

try:
    # Wait for boot to complete and shell prompt
    child.expect(PROMPT, timeout=60)
    print("\nBoot OK, running tests...\n", file=sys.stderr)

    tests = [
        ("timerfd", "/bin/test_timerfd"),
        ("sched", "/bin/test_sched"),
        ("msg", "/bin/test_msg"),
        ("ipv6", "/bin/test_ipv6"),
        ("reuseaddr", "/bin/test_reuseaddr"),
        ("syscall", "/bin/test_syscall"),
    ]

    for name, cmd in tests:
        print(f"\n{'='*60}", file=sys.stderr)
        print(f"Running: {name}", file=sys.stderr)
        print(f"{'='*60}", file=sys.stderr)

        child.sendline(cmd)
        # Wait for next prompt (command output comes before it)
        idx = child.expect([PROMPT, pexpect.TIMEOUT], timeout=30)

        # Get the output between command and prompt
        output = child.before
        if output:
            # Clean up the output
            lines = output.strip().split('\n')
            clean_lines = []
            for line in lines:
                line = line.strip()
                if not line or line == cmd:
                    continue
                clean_lines.append(line)
            clean_output = '\n'.join(clean_lines)

            print(clean_output, file=sys.stderr)

            passed = "ALL TESTS PASSED" in clean_output or "TEST COMPLETE" in clean_output
            RESULTS[name] = {"passed": passed, "output": clean_output}
        else:
            print("[NO OUTPUT]", file=sys.stderr)
            RESULTS[name] = {"passed": False, "output": ""}

    child.sendline("exit")
    child.expect(pexpect.EOF, timeout=10)

except pexpect.TIMEOUT:
    print("TIMEOUT: QEMU did not reach expected state", file=sys.stderr)
except Exception as e:
    print(f"ERROR: {e}", file=sys.stderr)
finally:
    try:
        child.terminate(force=True)
    except:
        pass

# Print summary
print("\n" + "=" * 60)
print("TEST RESULTS SUMMARY")
print("=" * 60)
all_pass = True
for name, result in RESULTS.items():
    status = "PASS" if result["passed"] else "FAIL"
    if not result["passed"]:
        all_pass = False
    print(f"  [{status}] {name}")

if all_pass:
    print("\n  ALL TESTS PASSED!")
else:
    print("\n  SOME TESTS FAILED!")
    sys.exit(1)
