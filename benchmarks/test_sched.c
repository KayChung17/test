// Test scheduler: sched_setscheduler, sched_getscheduler, getpriority
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sched.h>
#include <sys/resource.h>
#include <errno.h>
#include <string.h>

#define CHECK(cond, msg) do { \
    if (!(cond)) { fprintf(stderr, "FAIL: %s (errno=%d=%s)\n", msg, errno, strerror(errno)); return 1; } \
} while(0)

int main() {
    printf("=== scheduler test ===\n");

    int policy;
    struct sched_param param;

    // Get current scheduler policy
    policy = sched_getscheduler(0);
    CHECK(policy != -1, "sched_getscheduler(0)");
    printf("PASS: sched_getscheduler(0) -> policy=%d (SCHED_OTHER=%d, SCHED_FIFO=%d, SCHED_RR=%d)\n",
           policy, SCHED_OTHER, SCHED_FIFO, SCHED_RR);

    // Get current priority
    int prio = getpriority(PRIO_PROCESS, 0);
    CHECK(prio != -1 || errno == 0, "getpriority(PRIO_PROCESS, 0)");
    printf("PASS: getpriority -> %d\n", prio);

    // Set priority (nice value)
    int ret = setpriority(PRIO_PROCESS, 0, -5);
    CHECK(ret == 0, "setpriority(-5)");
    printf("PASS: setpriority(-5)\n");

    prio = getpriority(PRIO_PROCESS, 0);
    printf("PASS: verify priority -> %d\n", prio);

    // Test SCHED_FIFO
    param.sched_priority = 50;
    ret = sched_setscheduler(0, SCHED_FIFO, &param);
    CHECK(ret == 0, "sched_setscheduler(SCHED_FIFO, prio=50)");
    printf("PASS: sched_setscheduler(SCHED_FIFO, 50)\n");

    // Verify SCHED_FIFO was set
    policy = sched_getscheduler(0);
    CHECK(policy == SCHED_FIFO, "verify SCHED_FIFO");
    printf("PASS: verify policy -> SCHED_FIFO\n");

    int param_ret = sched_getparam(0, &param);
    CHECK(param_ret == 0, "sched_getparam");
    CHECK(param.sched_priority == 50, "verify priority=50");
    printf("PASS: sched_getparam -> prio=%d\n", param.sched_priority);

    // Test SCHED_RR
    param.sched_priority = 80;
    ret = sched_setscheduler(0, SCHED_RR, &param);
    CHECK(ret == 0, "sched_setscheduler(SCHED_RR, prio=80)");
    printf("PASS: sched_setscheduler(SCHED_RR, 80)\n");

    policy = sched_getscheduler(0);
    CHECK(policy == SCHED_RR, "verify SCHED_RR");
    printf("PASS: verify policy -> SCHED_RR\n");

    // Test SCHED_OTHER (back to normal)
    param.sched_priority = 0;
    ret = sched_setscheduler(0, SCHED_OTHER, &param);
    CHECK(ret == 0, "sched_setscheduler(SCHED_OTHER)");
    printf("PASS: sched_setscheduler(SCHED_OTHER)\n");

    // Test invalid priority
    param.sched_priority = 200;
    ret = sched_setscheduler(0, SCHED_FIFO, &param);
    CHECK(ret == -1 && errno == EINVAL, "invalid priority -> EINVAL");
    printf("PASS: invalid priority -> EINVAL\n");

    printf("\n=== scheduler: ALL TESTS PASSED ===\n");
    return 0;
}
