// Test timerfd: create, settime, gettime, read
// gcc -static test_timerfd.c -o test_timerfd
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/timerfd.h>
#include <time.h>
#include <stdint.h>
#include <string.h>
#include <errno.h>

#define CHECK(cond, msg) do { \
    if (!(cond)) { fprintf(stderr, "FAIL: %s (errno=%d)\n", msg, errno); return 1; } \
} while(0)

int main() {
    printf("=== timerfd test ===\n");

    // Create timerfd with CLOCK_MONOTONIC
    int fd = timerfd_create(CLOCK_MONOTONIC, TFD_NONBLOCK);
    CHECK(fd >= 0, "timerfd_create(CLOCK_MONOTONIC)");
    printf("PASS: timerfd_create(CLOCK_MONOTONIC) -> fd=%d\n", fd);

    // Create timerfd with CLOCK_REALTIME
    int fd2 = timerfd_create(CLOCK_REALTIME, TFD_CLOEXEC);
    CHECK(fd2 >= 0, "timerfd_create(CLOCK_REALTIME)");
    printf("PASS: timerfd_create(CLOCK_REALTIME) -> fd=%d\n", fd2);

    // Set a one-shot timer: 100ms from now
    struct itimerspec new_val = {
        .it_value = { .tv_sec = 0, .tv_nsec = 100000000 },      // 100ms
        .it_interval = { .tv_sec = 0, .tv_nsec = 0 },
    };
    struct itimerspec old_val;
    int ret = timerfd_settime(fd, 0, &new_val, &old_val);
    CHECK(ret == 0, "timerfd_settime(100ms)");
    CHECK(old_val.it_value.tv_sec == 0 && old_val.it_value.tv_nsec == 0, "old_value zero");
    printf("PASS: timerfd_settime(100ms)\n");

    // Get time before expiry - should show remaining <= 100ms
    struct itimerspec curr;
    ret = timerfd_gettime(fd, &curr);
    CHECK(ret == 0, "timerfd_gettime");
    CHECK(curr.it_value.tv_nsec > 0 || curr.it_value.tv_sec > 0,
          "remaining time > 0");
    printf("PASS: timerfd_gettime -> remaining=%ld.%09ld\n",
           (long)curr.it_value.tv_sec, (long)curr.it_value.tv_nsec);

    // Read before expiry should fail with EAGAIN (non-blocking)
    uint64_t expirations;
    ssize_t n = read(fd, &expirations, sizeof(expirations));
    CHECK(n < 0 && errno == EAGAIN, "read before expiry -> EAGAIN");
    printf("PASS: read before expiry -> EAGAIN\n");

    // Wait for expiry
    usleep(150000);  // 150ms

    // Now read should succeed
    n = read(fd, &expirations, sizeof(expirations));
    CHECK(n == sizeof(expirations), "read after expiry");
    CHECK(expirations >= 1, "at least 1 expiration");
    printf("PASS: read after expiry -> %lu expiration(s)\n", (unsigned long)expirations);

    // After one-shot, another read should fail
    n = read(fd, &expirations, sizeof(expirations));
    CHECK(n < 0 && errno == EAGAIN, "read after one-shot expiry -> EAGAIN");
    printf("PASS: one-shot timer disarmed after read\n");

    // Test recurring timer: 50ms interval, 50ms value
    struct itimerspec recur = {
        .it_value = { .tv_sec = 0, .tv_nsec = 50000000 },      // 50ms
        .it_interval = { .tv_sec = 0, .tv_nsec = 50000000 },    // 50ms
    };
    ret = timerfd_settime(fd, 0, &recur, NULL);
    CHECK(ret == 0, "timerfd_settime(recurring 50ms)");
    printf("PASS: timerfd_settime(recurring 50ms)\n");

    // Wait 200ms and read - should get ~4 expirations
    usleep(200000);
    n = read(fd, &expirations, sizeof(expirations));
    CHECK(n == sizeof(expirations), "read recurring timer");
    CHECK(expirations >= 3 && expirations <= 5,
          "expirations ~4 (3-5 acceptable)");
    printf("PASS: recurring timer -> %lu expirations\n", (unsigned long)expirations);

    // Test TFD_TIMER_ABSTIME
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    struct itimerspec abst = {
        .it_value = { .tv_sec = now.tv_sec, .tv_nsec = now.tv_nsec + 100000000 },
        .it_interval = { .tv_sec = 0, .tv_nsec = 0 },
    };
    // Normalize nsec overflow
    if (abst.it_value.tv_nsec >= 1000000000) {
        abst.it_value.tv_sec += 1;
        abst.it_value.tv_nsec -= 1000000000;
    }
    ret = timerfd_settime(fd, TFD_TIMER_ABSTIME, &abst, NULL);
    CHECK(ret == 0, "timerfd_settime(TFD_TIMER_ABSTIME)");
    printf("PASS: timerfd_settime(TFD_TIMER_ABSTIME)\n");

    usleep(150000);
    n = read(fd, &expirations, sizeof(expirations));
    CHECK(n == sizeof(expirations) && expirations >= 1, "read absolute timer");
    printf("PASS: absolute timer -> %lu expiration(s)\n", (unsigned long)expirations);

    close(fd);
    close(fd2);
    printf("\n=== timerfd: ALL TESTS PASSED ===\n");
    return 0;
}
