// Test IPv6 socket support: AF_INET6 socket, bind, connect
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
    printf("=== IPv6 socket test ===\n");

    // Create IPv6 TCP socket
    int sock = socket(AF_INET6, SOCK_STREAM, 0);
    CHECK(sock >= 0, "socket(AF_INET6, SOCK_STREAM)");
    printf("PASS: socket(AF_INET6, SOCK_STREAM) -> fd=%d\n", sock);
    close(sock);

    // Create IPv6 UDP socket
    int udp_sock = socket(AF_INET6, SOCK_DGRAM, 0);
    CHECK(udp_sock >= 0, "socket(AF_INET6, SOCK_DGRAM)");
    printf("PASS: socket(AF_INET6, SOCK_DGRAM) -> fd=%d\n", udp_sock);

    // Bind to IPv6 loopback
    struct sockaddr_in6 addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin6_family = AF_INET6;
    addr.sin6_port = htons(12345);
    addr.sin6_addr = in6addr_loopback;

    int ret = bind(udp_sock, (struct sockaddr*)&addr, sizeof(addr));
    CHECK(ret == 0, "bind(::1:12345)");
    printf("PASS: bind IPv6 loopback (::1:12345)\n");
    close(udp_sock);

    // Create IPv6 TCP socket and bind to different port
    int sock2 = socket(AF_INET6, SOCK_STREAM, 0);
    CHECK(sock2 >= 0, "socket(AF_INET6, SOCK_STREAM) #2");
    addr.sin6_port = htons(12346);
    ret = bind(sock2, (struct sockaddr*)&addr, sizeof(addr));
    CHECK(ret == 0, "bind IPv6 TCP (::1:12346)");
    printf("PASS: bind IPv6 TCP (::1:12346)\n");
    close(sock2);

    printf("\n=== IPv6: ALL TESTS PASSED ===\n");
    return 0;
}
