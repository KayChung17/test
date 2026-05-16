// Test SO_REUSEADDR: listen() collision detection
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <errno.h>
#include <string.h>

#define CHECK(cond, msg) do { \
    if (!(cond)) { fprintf(stderr, "FAIL: %s (errno=%d=%s)\n", msg, errno, strerror(errno)); return 1; } \
} while(0)

int main() {
    printf("=== SO_REUSEADDR test ===\n");

    int sock1 = socket(AF_INET, SOCK_STREAM, 0);
    CHECK(sock1 >= 0, "socket #1");

    // Set SO_REUSEADDR on first socket
    int optval = 1;
    int ret = setsockopt(sock1, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));
    CHECK(ret == 0, "setsockopt(SO_REUSEADDR) on sock1");
    printf("PASS: setsockopt(SO_REUSEADDR)\n");

    // Verify SO_REUSEADDR was set
    int verify = 0;
    socklen_t optlen = sizeof(verify);
    ret = getsockopt(sock1, SOL_SOCKET, SO_REUSEADDR, &verify, &optlen);
    CHECK(ret == 0 && verify == 1, "getsockopt(SO_REUSEADDR) -> 1");
    printf("PASS: getsockopt(SO_REUSEADDR) -> %d\n", verify);

    // Bind and listen first socket (registers port in listen table)
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port = htons(22222);

    ret = bind(sock1, (struct sockaddr*)&addr, sizeof(addr));
    CHECK(ret == 0, "bind #1");
    printf("PASS: bind #1 ok\n");

    ret = listen(sock1, 5);
    CHECK(ret == 0, "listen #1 with SO_REUSEADDR");
    printf("PASS: listen #1 with SO_REUSEADDR ok\n");

    // Second socket without SO_REUSEADDR: listen should fail with EADDRINUSE
    int sock2 = socket(AF_INET, SOCK_STREAM, 0);
    CHECK(sock2 >= 0, "socket #2");
    ret = bind(sock2, (struct sockaddr*)&addr, sizeof(addr));
    // bind may or may not fail; the real check is at listen()
    ret = listen(sock2, 5);
    CHECK(ret < 0 && errno == EADDRINUSE,
          "listen #2 without SO_REUSEADDR -> EADDRINUSE");
    printf("PASS: listen #2 without SO_REUSEADDR -> EADDRINUSE\n");
    close(sock2);

    // Third socket WITH SO_REUSEADDR: listen should succeed
    int sock3 = socket(AF_INET, SOCK_STREAM, 0);
    CHECK(sock3 >= 0, "socket #3");
    ret = setsockopt(sock3, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));
    CHECK(ret == 0, "setsockopt(SO_REUSEADDR) on sock3");
    ret = bind(sock3, (struct sockaddr*)&addr, sizeof(addr));
    // bind might fail here too
    ret = listen(sock3, 5);
    CHECK(ret == 0, "listen #3 with SO_REUSEADDR");
    printf("PASS: listen #3 with SO_REUSEADDR -> ok\n");

    close(sock1);
    close(sock3);

    printf("\n=== SO_REUSEADDR: ALL TESTS PASSED ===\n");
    return 0;
}
