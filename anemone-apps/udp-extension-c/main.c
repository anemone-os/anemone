#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define RESOLVER_NAME "anemone-stage2b.test"
#define RESOLVER_ADDRESS "198.51.100.42"

struct result {
    int passed;
    int failed;
};

static int fail(const char *case_name, const char *reason) {
    printf("UDPEXT-C:FAIL:%s:%s\n", case_name, reason);
    return -1;
}

static int expect_errno(const char *case_name, int actual, int expected) {
    if (actual == expected)
        return 0;
    fprintf(stderr, "UDPEXT-C:ERRNO:%s:expected=%d:actual=%d\n", case_name, expected, actual);
    return -1;
}

static int make_udp(void) {
    return socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, IPPROTO_UDP);
}

static struct sockaddr_in ipv4(const char *address, uint16_t port) {
    struct sockaddr_in value = {
        .sin_family = AF_INET,
        .sin_port = htons(port),
    };
    inet_pton(AF_INET, address, &value.sin_addr);
    return value;
}

static int bind_loopback(int fd, struct sockaddr_in *name) {
    *name = ipv4("127.0.0.1", 0);
    if (bind(fd, (const struct sockaddr *)name, sizeof(*name)) < 0)
        return -1;
    socklen_t length = sizeof(*name);
    return getsockname(fd, (struct sockaddr *)name, &length);
}

static int wait_readable(int fd, int timeout_ms) {
    struct pollfd waiter = {.fd = fd, .events = POLLIN};
    int result;
    do {
        result = poll(&waiter, 1, timeout_ms);
    } while (result < 0 && errno == EINTR);
    return result == 1 && (waiter.revents & (POLLIN | POLLHUP));
}

static void terminate_and_reap(pid_t child) {
    if (kill(child, SIGTERM) < 0 && errno != ESRCH)
        fprintf(stderr, "UDPEXT-C:DNS:kill-failed:errno=%d\n", errno);
    while (waitpid(child, NULL, 0) < 0 && errno == EINTR) {
    }
}

static int reap_child(pid_t child, int *status) {
    pid_t result;
    do {
        result = waitpid(child, status, 0);
    } while (result < 0 && errno == EINTR);
    return result == child ? 0 : -1;
}

static int send_message(int fd, const struct sockaddr_in *destination, const char *left,
                        const char *right) {
    struct iovec vectors[2] = {
        {.iov_base = (void *)left, .iov_len = strlen(left)},
        {.iov_base = (void *)right, .iov_len = strlen(right)},
    };
    struct msghdr message = {
        .msg_name = (void *)destination,
        .msg_namelen = sizeof(*destination),
        .msg_iov = vectors,
        .msg_iovlen = 2,
    };
    return sendmsg(fd, &message, MSG_NOSIGNAL);
}

static int receive_message(int fd, char *payload, size_t payload_size,
                           struct sockaddr_in *peer, int flags, int *message_flags) {
    struct iovec vector = {.iov_base = payload, .iov_len = payload_size};
    struct msghdr message = {
        .msg_name = peer,
        .msg_namelen = sizeof(*peer),
        .msg_iov = &vector,
        .msg_iovlen = 1,
    };
    int result = recvmsg(fd, &message, flags);
    if (result >= 0 && message_flags != NULL)
        *message_flags = message.msg_flags;
    return result;
}

static int case_connected_and_unconnected(void) {
    int server = -1;
    int second = -1;
    int client = -1;
    struct sockaddr_in first_name;
    struct sockaddr_in second_name;
    char payload[32] = {0};
    struct sockaddr_in peer;
    int result = -1;

    server = make_udp();
    second = make_udp();
    client = make_udp();
    if (server < 0 || second < 0 || client < 0)
        goto out;
    if (bind_loopback(server, &first_name) < 0 || bind_loopback(second, &second_name) < 0)
        goto out;
    if (send_message(client, &first_name, "c-", "explicit") != 10)
        goto out;
    if (receive_message(server, payload, sizeof(payload), &peer, 0, NULL) != 10 ||
        memcmp(payload, "c-explicit", 10) != 0)
        goto out;
    if (send_message(server, &peer, "u-", "reply") != 7)
        goto out;
    memset(payload, 0, sizeof(payload));
    if (receive_message(client, payload, sizeof(payload), &peer, 0, NULL) != 7 ||
        memcmp(payload, "u-reply", 7) != 0 || peer.sin_port != first_name.sin_port)
        goto out;
    if (connect(client, (const struct sockaddr *)&first_name, sizeof(first_name)) < 0)
        goto out;
    if (send_message(client, NULL, "c-", "default") != 9)
        goto out;
    if (receive_message(server, payload, sizeof(payload), &peer, 0, NULL) != 9 ||
        memcmp(payload, "c-default", 9) != 0)
        goto out;
    if (send_message(server, &peer, "c-", "reply") != 7)
        goto out;
    memset(payload, 0, sizeof(payload));
    if (receive_message(client, payload, sizeof(payload), &peer, 0, NULL) != 7 ||
        memcmp(payload, "c-reply", 7) != 0 || peer.sin_port != first_name.sin_port)
        goto out;
    if (send_message(client, &second_name, "c-", "override") != 10)
        goto out;
    if (receive_message(second, payload, sizeof(payload), &peer, 0, NULL) != 10 ||
        memcmp(payload, "c-override", 10) != 0)
        goto out;
    struct sockaddr_in observed;
    socklen_t observed_length = sizeof(observed);
    if (getpeername(client, (struct sockaddr *)&observed, &observed_length) < 0 ||
        observed.sin_port != first_name.sin_port)
        goto out;
    int unconnected = make_udp();
    if (unconnected < 0)
        goto out;
    struct iovec vector = {.iov_base = (void *)"missing", .iov_len = 7};
    struct msghdr missing = {.msg_iov = &vector, .msg_iovlen = 1};
    errno = 0;
    if (sendmsg(unconnected, &missing, MSG_NOSIGNAL) >= 0 ||
        expect_errno("unconnected-default", errno, EDESTADDRREQ) < 0)
        goto close_unconnected;
    result = 0;
close_unconnected:
    close(unconnected);
out:
    if (result < 0)
        fail("connected-unconnected", "request-reply");
    if (client >= 0)
        close(client);
    if (second >= 0)
        close(second);
    if (server >= 0)
        close(server);
    return result;
}

static int case_flags_and_control(void) {
    int fd = -1;
    int server = -1;
    int client = -1;
    struct sockaddr_in server_name;
    int ok = 0;
    fd = make_udp();
    server = make_udp();
    client = make_udp();
    if (fd < 0 || server < 0 || client < 0 || bind_loopback(server, &server_name) < 0)
        goto out;
    struct iovec vector = {.iov_base = (void *)"control", .iov_len = 7};
    char control[8] = {0};
    struct msghdr message = {
        .msg_iov = &vector,
        .msg_iovlen = 1,
        .msg_control = control,
        .msg_controllen = sizeof(control),
    };
    errno = 0;
    int result = sendmsg(fd, &message, MSG_NOSIGNAL);
    ok = result < 0 && expect_errno("control", errno, EOPNOTSUPP) == 0;
    message.msg_control = NULL;
    message.msg_controllen = 0;
    errno = 0;
    result = sendmsg(fd, &message, MSG_NOSIGNAL | 1);
    ok = ok && result < 0 && expect_errno("send-flag", errno, EOPNOTSUPP) == 0;
    char payload[8] = {0};
    struct iovec receive_vector = {.iov_base = payload, .iov_len = sizeof(payload)};
    char receive_control[8] = {0};
    struct msghdr receive = {
        .msg_iov = &receive_vector,
        .msg_iovlen = 1,
        .msg_control = receive_control,
        .msg_controllen = sizeof(receive_control),
        .msg_flags = -1,
    };
    if (sendto(client, "empty", 5, 0, (const struct sockaddr *)&server_name,
               sizeof(server_name)) != 5 ||
        recvmsg(server, &receive, 0) != 5 || memcmp(payload, "empty", 5) != 0 ||
        receive.msg_controllen != 0 || receive.msg_flags != 0)
        ok = 0;
    receive.msg_control = NULL;
    receive.msg_controllen = 0;
    errno = 0;
    result = recvmsg(server, &receive, MSG_OOB);
    ok = ok && result < 0 && expect_errno("recv-flag", errno, EOPNOTSUPP) == 0;
out:
    if (client >= 0)
        close(client);
    if (server >= 0)
        close(server);
    if (fd >= 0)
        close(fd);
    return ok ? 0 : fail("flags-control", "rejection");
}

static int case_peek_truncate_and_zero(void) {
    int server = make_udp();
    int client = make_udp();
    struct sockaddr_in server_name;
    char short_payload[2] = {0};
    struct sockaddr_in peer;
    int flags = 0;
    int result = -1;
    if (server < 0 || client < 0 || bind_loopback(server, &server_name) < 0)
        goto out;
    if (sendto(client, "truncate", 8, 0, (const struct sockaddr *)&server_name, sizeof(server_name)) != 8)
        goto out;
    if (receive_message(server, short_payload, sizeof(short_payload), &peer, MSG_PEEK, &flags) != 2 ||
        flags != MSG_TRUNC || memcmp(short_payload, "tr", 2) != 0)
        goto out;
    memset(short_payload, 0, sizeof(short_payload));
    if (receive_message(server, short_payload, sizeof(short_payload), &peer, MSG_TRUNC, &flags) != 8 ||
        flags != MSG_TRUNC || memcmp(short_payload, "tr", 2) != 0)
        goto out;
    if (sendto(client, "", 0, 0, (const struct sockaddr *)&server_name, sizeof(server_name)) != 0)
        goto out;
    if (receive_message(server, NULL, 0, &peer, 0, &flags) != 0 || flags != 0)
        goto out;
    result = 0;
out:
    if (result < 0)
        fail("peek-truncate-zero", "datagram-boundary");
    if (client >= 0)
        close(client);
    if (server >= 0)
        close(server);
    return result;
}

static int case_fault_ordering(void) {
    int server = make_udp();
    int client = make_udp();
    struct sockaddr_in server_name;
    char prefix[2] = {0};
    void *protected = MAP_FAILED;
    int result = -1;
    if (server < 0 || client < 0 || bind_loopback(server, &server_name) < 0)
        goto out;
    if (sendto(client, "fault", 5, 0, (const struct sockaddr *)&server_name, sizeof(server_name)) != 5)
        goto out;
    long page_size = sysconf(_SC_PAGESIZE);
    if (page_size <= 0)
        goto out;
    protected = mmap(NULL, (size_t)page_size, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (protected == MAP_FAILED || mprotect(protected, (size_t)page_size, PROT_NONE) < 0)
        goto out;
    /* Preserve a valid imported range whose later segment faults during copyout. */
    struct iovec vectors[2] = {
        {.iov_base = prefix, .iov_len = sizeof(prefix)},
        {.iov_base = protected, .iov_len = 3},
    };
    struct msghdr message = {
        .msg_iov = vectors,
        .msg_iovlen = 2,
    };
    struct pollfd waiter = {.fd = server, .events = POLLIN};
    if (poll(&waiter, 1, 5000) != 1 || !(waiter.revents & POLLIN))
        goto out;
    errno = 0;
    ssize_t received = recvmsg(server, &message, MSG_DONTWAIT);
    if (received >= 0 || errno != EFAULT || memcmp(prefix, "fa", 2) != 0) {
        fprintf(stderr, "UDPEXT-C:FAULT:consume:return=%zd:errno=%d:prefix=%02x%02x\n",
                received, errno, (unsigned char)prefix[0], (unsigned char)prefix[1]);
        goto out;
    }
    if (sendto(client, "peek-fault", 10, 0, (const struct sockaddr *)&server_name, sizeof(server_name)) != 10)
        goto out;
    memset(prefix, 0, sizeof(prefix));
    message.msg_flags = 0;
    waiter.revents = 0;
    if (poll(&waiter, 1, 5000) != 1 || !(waiter.revents & POLLIN))
        goto out;
    errno = 0;
    received = recvmsg(server, &message, MSG_DONTWAIT | MSG_PEEK);
    if (received >= 0 || errno != EFAULT || memcmp(prefix, "pe", 2) != 0) {
        fprintf(stderr, "UDPEXT-C:FAULT:peek:return=%zd:errno=%d:prefix=%02x%02x\n",
                received, errno, (unsigned char)prefix[0], (unsigned char)prefix[1]);
        goto out;
    }
    char payload[16] = {0};
    if (recvfrom(server, payload, sizeof(payload), 0, NULL, NULL) != 10 || memcmp(payload, "peek-fault", 10) != 0)
        goto out;
    result = 0;
out:
    if (result < 0)
        fail("fault-ordering", "copyout-consume");
    if (protected != MAP_FAILED)
        munmap(protected, (size_t)page_size);
    if (client >= 0)
        close(client);
    if (server >= 0)
        close(server);
    return result;
}

static int case_header_output_ordering(void) {
    int server = make_udp();
    int client = make_udp();
    struct sockaddr_in server_name;
    char payload[16] = {0};
    struct iovec vector = {.iov_base = payload, .iov_len = sizeof(payload)};
    struct msghdr message = {
        .msg_name = (void *)1,
        .msg_namelen = sizeof(struct sockaddr_in),
        .msg_iov = &vector,
        .msg_iovlen = 1,
        .msg_controllen = 9,
        .msg_flags = -1,
    };
    int result = -1;
    if (server < 0 || client < 0 || bind_loopback(server, &server_name) < 0)
        goto out;
    if (sendto(client, "name-fault", 10, 0, (const struct sockaddr *)&server_name,
               sizeof(server_name)) != 10)
        goto out;
    errno = 0;
    ssize_t received = recvmsg(server, &message, 0);
    if (received >= 0 || errno != EFAULT || memcmp(payload, "name-fault", 10) != 0 ||
        message.msg_flags != -1 || message.msg_controllen != 9)
        goto out;
    errno = 0;
    if (recvfrom(server, payload, sizeof(payload), MSG_DONTWAIT, NULL, NULL) >= 0 ||
        errno != EAGAIN)
        goto out;
    if (sendto(client, "peek-name", 9, 0, (const struct sockaddr *)&server_name,
               sizeof(server_name)) != 9)
        goto out;
    memset(payload, 0, sizeof(payload));
    message.msg_flags = -1;
    message.msg_controllen = 9;
    errno = 0;
    received = recvmsg(server, &message, MSG_PEEK);
    if (received >= 0 || errno != EFAULT || memcmp(payload, "peek-name", 9) != 0 ||
        message.msg_flags != -1 || message.msg_controllen != 9)
        goto out;
    memset(payload, 0, sizeof(payload));
    if (recvfrom(server, payload, sizeof(payload), 0, NULL, NULL) != 9 ||
        memcmp(payload, "peek-name", 9) != 0)
        goto out;
    result = 0;
out:
    if (result < 0)
        fail("header-output-ordering", "name-fault");
    if (client >= 0)
        close(client);
    if (server >= 0)
        close(server);
    return result;
}

static int case_lifecycle(void) {
    int server = make_udp();
    int alias = -1;
    struct sockaddr_in server_name;
    int result = -1;
    if (server < 0 || bind_loopback(server, &server_name) < 0)
        goto out;
    alias = dup(server);
    if (alias < 0 || close(server) < 0)
        goto out;
    server = -1;
    char byte = 0;
    errno = 0;
    result = recvfrom(alias, &byte, 1, MSG_DONTWAIT, NULL, NULL);
    result = result < 0 && errno == EAGAIN ? 0 : -1;
out:
    if (alias >= 0)
        close(alias);
    if (server >= 0)
        close(server);
    return result == 0 ? 0 : fail("lifecycle", "alias");
}

static int read_dns_name(const unsigned char *packet, size_t length, size_t *offset,
                         char *name, size_t name_size) {
    size_t out = 0;
    while (*offset < length) {
        unsigned int label_length = packet[(*offset)++];
        if (label_length == 0) {
            if (out == 0 || out >= name_size)
                return -1;
            name[out] = '\0';
            return 0;
        }
        if (label_length > 63 || *offset + label_length > length)
            return -1;
        if (out != 0) {
            if (out + 1 >= name_size)
                return -1;
            name[out++] = '.';
        }
        if (out + label_length >= name_size)
            return -1;
        memcpy(name + out, packet + *offset, label_length);
        out += label_length;
        *offset += label_length;
    }
    return -1;
}

static int serve_dns_once(int ready_fd, int completion_fd) {
    int fd = make_udp();
    struct sockaddr_in local = ipv4("127.0.0.1", 53);
    char ready = 'e';
    if (fd < 0 || bind(fd, (const struct sockaddr *)&local, sizeof(local)) < 0) {
        fprintf(stderr, "UDPEXT-C:DNS:bind-failed:errno=%d\n", errno);
        write(ready_fd, &ready, 1);
        if (fd >= 0)
            close(fd);
        return -1;
    }
    ready = 'r';
    fprintf(stderr, "UDPEXT-C:DNS:READY\n");
    if (write(ready_fd, &ready, 1) != 1) {
        close(fd);
        return -1;
    }
    unsigned char request[512];
    struct sockaddr_in source;
    socklen_t source_length = sizeof(source);
    if (!wait_readable(fd, 5000)) {
        fprintf(stderr, "UDPEXT-C:DNS:request-timeout\n");
        close(fd);
        return -1;
    }
    ssize_t received = recvfrom(fd, request, sizeof(request), 0,
                                (struct sockaddr *)&source, &source_length);
    if (received < 16) {
        fprintf(stderr, "UDPEXT-C:DNS:recv-failed:return=%zd:errno=%d\n", received, errno);
        close(fd);
        return -1;
    }
    size_t offset = 12;
    char name[128];
    if (read_dns_name(request, (size_t)received, &offset, name, sizeof(name)) < 0) {
        fprintf(stderr, "UDPEXT-C:DNS:name-parse-failed:length=%zd\n", received);
        close(fd);
        return -1;
    }
    if (offset + 4 > (size_t)received) {
        fprintf(stderr, "UDPEXT-C:DNS:question-truncated:name=%s:length=%zd\n", name, received);
        close(fd);
        return -1;
    }
    if (strcmp(name, RESOLVER_NAME) != 0 ||
        request[offset] != 0 || request[offset + 1] != 1 ||
        request[offset + 2] != 0 || request[offset + 3] != 1) {
        fprintf(stderr,
                "UDPEXT-C:DNS:query-rejected:name=%s:length=%zd:type=%02x%02x:class=%02x%02x\n",
                name, received, request[offset], request[offset + 1],
                request[offset + 2], request[offset + 3]);
        close(fd);
        return -1;
    }
    size_t question_end = offset + 4;
    unsigned char response[512];
    if (question_end + 16 > sizeof(response)) {
        close(fd);
        return -1;
    }
    memcpy(response, request, question_end);
    response[2] = 0x81;
    response[3] = 0x80;
    response[4] = 0;
    response[5] = 1;
    response[6] = 0;
    response[7] = 1;
    memset(response + 8, 0, 4);
    static const unsigned char answer[] = {
        0xc0, 0x0c, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x1e, 0x00, 0x04, 0xc6, 0x33, 0x64, 0x2a,
    };
    memcpy(response + question_end, answer, sizeof(answer));
    ssize_t sent = sendto(fd, response, question_end + sizeof(answer), 0,
                          (const struct sockaddr *)&source, source_length);
    if (sent < 0)
        fprintf(stderr, "UDPEXT-C:DNS:response-failed:errno=%d\n", errno);
    else
        fprintf(stderr, "UDPEXT-C:DNS:response:return=%zd\n", sent);
    char completed = 0;
    ssize_t completion_length = -1;
    if (sent == (ssize_t)(question_end + sizeof(answer))) {
        /* Keep the nameserver identity alive while musl validates and parses the reply. */
        if (wait_readable(completion_fd, 5000))
            completion_length = read(completion_fd, &completed, 1);
        else
            fprintf(stderr, "UDPEXT-C:DNS:completion-timeout\n");
    }
    close(fd);
    return sent == (ssize_t)(question_end + sizeof(answer)) &&
                   completion_length == 1 && completed == 'd'
               ? 0
               : -1;
}

static int case_resolver(void) {
    int ready_pipe[2];
    int completion_pipe[2];
    if (pipe(ready_pipe) < 0)
        return fail("resolver", "pipe");
    if (pipe(completion_pipe) < 0) {
        close(ready_pipe[0]);
        close(ready_pipe[1]);
        return fail("resolver", "pipe");
    }
    pid_t child = fork();
    if (child < 0) {
        close(ready_pipe[0]);
        close(ready_pipe[1]);
        close(completion_pipe[0]);
        close(completion_pipe[1]);
        return fail("resolver", "fork");
    }
    if (child == 0) {
        close(ready_pipe[0]);
        close(completion_pipe[1]);
        int result = serve_dns_once(ready_pipe[1], completion_pipe[0]);
        close(ready_pipe[1]);
        close(completion_pipe[0]);
        _exit(result == 0 ? 0 : 1);
    }
    close(ready_pipe[1]);
    close(completion_pipe[0]);
    char ready = 0;
    ssize_t ready_length = wait_readable(ready_pipe[0], 5000)
                               ? read(ready_pipe[0], &ready, 1)
                               : -1;
    close(ready_pipe[0]);
    if (ready_length != 1 || ready != 'r') {
        close(completion_pipe[1]);
        terminate_and_reap(child);
        return fail("resolver", "fixture-ready");
    }
    struct addrinfo hints = {
        .ai_family = AF_INET,
        .ai_socktype = SOCK_DGRAM,
        .ai_protocol = IPPROTO_UDP,
    };
    struct addrinfo *answers = NULL;
    int result = getaddrinfo(RESOLVER_NAME, NULL, &hints, &answers);
    char completed = 'd';
    ssize_t completion_length = write(completion_pipe[1], &completed, 1);
    close(completion_pipe[1]);
    if (result != 0) {
        fprintf(stderr, "UDPEXT-C:RESOLVER:FAIL:gai=%d:%s\n", result, gai_strerror(result));
        terminate_and_reap(child);
        return -1;
    }
    int matched = 0;
    for (struct addrinfo *item = answers; item != NULL; item = item->ai_next) {
        const struct sockaddr_in *address = (const struct sockaddr_in *)item->ai_addr;
        char text[INET_ADDRSTRLEN] = {0};
        inet_ntop(AF_INET, &address->sin_addr, text, sizeof(text));
        if (strcmp(text, RESOLVER_ADDRESS) == 0)
            matched = 1;
    }
    freeaddrinfo(answers);
    int status = 0;
    if (completion_length != 1) {
        terminate_and_reap(child);
        return fail("resolver", "fixture-completion");
    }
    if (reap_child(child, &status) < 0) {
        terminate_and_reap(child);
        return fail("resolver", "fixture-reap");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return fail("resolver", "fixture-result");
    if (!matched)
        return fail("resolver", "answer");
    printf("UDPEXT-C:RESOLVER:PASS:%s=%s\n", RESOLVER_NAME, RESOLVER_ADDRESS);
    return 0;
}

static void run_case(struct result *summary, const char *name, int (*test)(void)) {
    if (test() == 0) {
        summary->passed++;
        printf("UDPEXT-C:PASS:%s\n", name);
    } else {
        summary->failed++;
    }
}

int main(int argc, char **argv) {
    if (argc != 2 || strcmp(argv[1], "--local") != 0) {
        fprintf(stderr, "usage: %s --local\n", argv[0]);
        return 2;
    }
    /* Preserve resolver diagnostics if its child closes the completion pipe first. */
    if (signal(SIGPIPE, SIG_IGN) == SIG_ERR)
        return fail("resolver", "sigpipe");
    struct result summary = {0};
    printf("UDPEXT-C:START:local\n");
    run_case(&summary, "connected-unconnected", case_connected_and_unconnected);
    run_case(&summary, "flags-control", case_flags_and_control);
    run_case(&summary, "peek-truncate-zero", case_peek_truncate_and_zero);
    run_case(&summary, "fault-ordering", case_fault_ordering);
    run_case(&summary, "header-output-ordering", case_header_output_ordering);
    run_case(&summary, "lifecycle", case_lifecycle);
    run_case(&summary, "resolver", case_resolver);
    printf("UDPEXT-C:LOCAL-SUMMARY:passed=%d:failed=%d\n", summary.passed, summary.failed);
    return summary.failed == 0 ? 0 : 1;
}
