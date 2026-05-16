// Measure syscall throughput
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <time.h>
#include <sys/times.h>

#define ITERATIONS 100000

int main() {
    printf("=== syscall throughput test ===\n");
    printf("Running %d getpid() calls...\n", ITERATIONS);

    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);

    for (int i = 0; i < ITERATIONS; i++) {
        getpid();
    }

    clock_gettime(CLOCK_MONOTONIC, &end);

    long long elapsed_ns = (end.tv_sec - start.tv_sec) * 1000000000LL
                         + (end.tv_nsec - start.tv_nsec);
    double ns_per_call = (double)elapsed_ns / ITERATIONS;
    double calls_per_sec = 1e9 / ns_per_call;

    printf("Total time: %lld ns\n", elapsed_ns);
    printf("Per call:   %.1f ns\n", ns_per_call);
    printf("Throughput: %.0f calls/sec\n", calls_per_sec);

    // Also test getuid (another simple syscall)
    printf("\nRunning %d getuid() calls...\n", ITERATIONS);
    clock_gettime(CLOCK_MONOTONIC, &start);

    for (int i = 0; i < ITERATIONS; i++) {
        getuid();
    }

    clock_gettime(CLOCK_MONOTONIC, &end);
    elapsed_ns = (end.tv_sec - start.tv_sec) * 1000000000LL
               + (end.tv_nsec - start.tv_nsec);
    ns_per_call = (double)elapsed_ns / ITERATIONS;
    calls_per_sec = 1e9 / ns_per_call;

    printf("Total time: %lld ns\n", elapsed_ns);
    printf("Per call:   %.1f ns\n", ns_per_call);
    printf("Throughput: %.0f calls/sec\n", calls_per_sec);

    // Also test clock_gettime
    printf("\nRunning %d clock_gettime() calls...\n", ITERATIONS);
    clock_gettime(CLOCK_MONOTONIC, &start);

    for (int i = 0; i < ITERATIONS; i++) {
        clock_gettime(CLOCK_MONOTONIC, &end);
    }

    clock_gettime(CLOCK_MONOTONIC, &end);  // overwrites last loop result, but measurement is the loop
    // Actually, let's redo this properly
    struct timespec t1, t2;
    clock_gettime(CLOCK_MONOTONIC, &t1);
    for (int i = 0; i < ITERATIONS; i++) {
        clock_gettime(CLOCK_MONOTONIC, &t2);
    }
    clock_gettime(CLOCK_MONOTONIC, &end);
    elapsed_ns = (end.tv_sec - t1.tv_sec) * 1000000000LL
               + (end.tv_nsec - t1.tv_nsec);
    ns_per_call = (double)elapsed_ns / ITERATIONS;
    calls_per_sec = 1e9 / ns_per_call;

    printf("Total time: %lld ns\n", elapsed_ns);
    printf("Per call:   %.1f ns\n", ns_per_call);
    printf("Throughput: %.0f calls/sec\n", calls_per_sec);

    printf("\n=== syscall: TEST COMPLETE ===\n");
    return 0;
}
