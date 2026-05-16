// Test message queue: msgget, msgsnd(blocking), msgrcv(blocking), msgctl
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/msg.h>
#include <sys/ipc.h>
#include <sys/wait.h>
#include <errno.h>
#include <string.h>

#define CHECK(cond, msg) do { \
    if (!(cond)) { fprintf(stderr, "FAIL: %s (errno=%d=%s)\n", msg, errno, strerror(errno)); return 1; } \
} while(0)

struct test_msg {
    long mtype;
    char mtext[64];
};

int main() {
    printf("=== message queue test ===\n");

    key_t key = ftok("/tmp", 'M');
    CHECK(key != -1, "ftok");
    printf("PASS: ftok -> key=%d\n", key);

    // Create message queue
    int msqid = msgget(key, IPC_CREAT | 0666);
    CHECK(msqid >= 0, "msgget(IPC_CREAT)");
    printf("PASS: msgget -> msqid=%d\n", msqid);

    // Simple non-blocking send
    struct test_msg snd_msg = { .mtype = 1, .mtext = "hello" };
    int ret = msgsnd(msqid, &snd_msg, 5, IPC_NOWAIT);
    CHECK(ret == 0, "msgsnd(IPC_NOWAIT)");
    printf("PASS: msgsnd non-blocking\n");

    // Non-blocking receive
    struct test_msg rcv_msg;
    ssize_t n = msgrcv(msqid, &rcv_msg, sizeof(rcv_msg.mtext), 1, IPC_NOWAIT);
    CHECK(n == 5, "msgrcv(IPC_NOWAIT) got 5 bytes");
    CHECK(rcv_msg.mtype == 1, "msgrcv type=1");
    CHECK(strncmp(rcv_msg.mtext, "hello", 5) == 0, "msgrcv content='hello'");
    printf("PASS: msgrcv non-blocking -> '%5s'\n", rcv_msg.mtext);

    // Blocking receive from child process
    pid_t pid = fork();
    CHECK(pid >= 0, "fork");

    if (pid == 0) {
        // Child: wait a bit then send
        usleep(100000);  // 100ms
        struct test_msg child_msg = { .mtype = 2, .mtext = "from-child" };
        int r = msgsnd(msqid, &child_msg, 10, 0);  // blocking
        if (r != 0) {
            fprintf(stderr, "child msgsnd failed: %s\n", strerror(errno));
            exit(1);
        }
        exit(0);
    } else {
        // Parent: do blocking receive
        printf("PASS: blocking msgrcv waiting for child...\n");
        n = msgrcv(msqid, &rcv_msg, sizeof(rcv_msg.mtext), 2, 0);  // blocking
        CHECK(n == 10, "blocking msgrcv got 10 bytes");
        CHECK(rcv_msg.mtype == 2, "msgrcv type=2");
        CHECK(strncmp(rcv_msg.mtext, "from-child", 10) == 0, "msgrcv content='from-child'");
        printf("PASS: blocking msgrcv -> '%10s'\n", rcv_msg.mtext);
        waitpid(pid, NULL, 0);
    }

    // Non-blocking recv on empty queue
    n = msgrcv(msqid, &rcv_msg, sizeof(rcv_msg.mtext), 3, IPC_NOWAIT);
    CHECK(n < 0 && errno == ENOMSG, "msgrcv(IPC_NOWAIT) on empty -> ENOMSG");
    printf("PASS: msgrcv on empty queue -> ENOMSG\n");

    // Remove the queue
    ret = msgctl(msqid, IPC_RMID, NULL);
    CHECK(ret == 0, "msgctl(IPC_RMID)");
    printf("PASS: msgctl(IPC_RMID)\n");

    printf("\n=== message queue: ALL TESTS PASSED ===\n");
    return 0;
}
