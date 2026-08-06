#define _GNU_SOURCE

/*
 * Focused RV64 dual-libc oracle for the net-tcp Stage 4 candidate.
 *
 * The same source is statically linked with the competition image's glibc and
 * musl toolchains.  It exercises only the accepted IPv4 TCP R0 envelope and is
 * a validation asset, not a production dependency.
 */

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static unsigned char large_payload[1024 * 1024];
static volatile sig_atomic_t sigpipe_count;

static int fail(const char *step)
{
	fprintf(stderr, "TFAIL: tcp_r0_oracle step=%s errno=%d\n", step, errno);
	return 1;
}

static int expect_errno(long result, int expected, const char *step)
{
	if (result == -1 && errno == expected)
		return 0;
	fprintf(stderr,
		"TFAIL: tcp_r0_oracle step=%s result=%ld errno=%d expected=%d\n",
		step, result, errno, expected);
	return 1;
}

static void handle_sigpipe(int signal_number)
{
	(void)signal_number;
	sigpipe_count++;
}

static int wait_poll(int fd, short events, short required, const char *step)
{
	struct pollfd pfd = { .fd = fd, .events = events };
	int result = poll(&pfd, 1, 2000);

	if (result == 1 && (pfd.revents & required) == required)
		return 0;
	fprintf(stderr,
		"TFAIL: tcp_r0_oracle step=%s result=%d revents=0x%x required=0x%x errno=%d\n",
		step, result, (unsigned short)pfd.revents,
		(unsigned short)required, errno);
	return 1;
}

static int make_listener(struct sockaddr_in *address)
{
	socklen_t length = sizeof(*address);
	socklen_t option_length = sizeof(int);
	int enabled = 1;
	int queried = 0;
	int listener = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);

	if (listener < 0)
		return -1;
	memset(address, 0, sizeof(*address));
	address->sin_family = AF_INET;
	address->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
	if (setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &enabled,
			       sizeof(enabled)) < 0 ||
	    getsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &queried,
		       &option_length) < 0 || queried != 1 ||
	    bind(listener, (struct sockaddr *)address, sizeof(*address)) < 0 ||
	    getsockname(listener, (struct sockaddr *)address, &length) < 0 ||
	    address->sin_port == 0 || listen(listener, 10) < 0) {
		close(listener);
		return -1;
	}
	return listener;
}

static int creation_and_rejection_oracle(void)
{
	socklen_t length;
	int value;
	int fd;

	errno = 0;
	if (expect_errno(socket(AF_INET, SOCK_STREAM, IPPROTO_UDP),
			 EPROTONOSUPPORT, "invalid-protocol") ||
	    expect_errno(socket(AF_INET, SOCK_STREAM | 0x40000000, 0), EINVAL,
			 "invalid-type-flag"))
		return 1;

	fd = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
	if (fd < 0)
		return fail("creation-flags");
	if ((fcntl(fd, F_GETFL) & O_NONBLOCK) == 0 ||
	    (fcntl(fd, F_GETFD) & FD_CLOEXEC) == 0)
		return fail("creation-flags-visible");
	length = sizeof(value);
	if (getsockopt(fd, SOL_SOCKET, SO_DOMAIN, &value, &length) < 0 ||
	    value != AF_INET)
		return fail("query-domain");
	length = sizeof(value);
	if (getsockopt(fd, SOL_SOCKET, SO_TYPE, &value, &length) < 0 ||
	    value != SOCK_STREAM)
		return fail("query-type");
	length = sizeof(value);
	if (getsockopt(fd, SOL_SOCKET, SO_PROTOCOL, &value, &length) < 0 ||
	    value != IPPROTO_TCP)
		return fail("query-protocol");
	close(fd);
	return 0;
}

static int readiness_oracle(int fd)
{
	struct epoll_event event = { .events = EPOLLIN, .data.fd = fd };
	struct epoll_event output;
	struct timeval timeout = { .tv_sec = 2, .tv_usec = 0 };
	fd_set readfds;
	int epfd;

	if (wait_poll(fd, POLLIN, POLLIN, "poll-readable"))
		return 1;
	FD_ZERO(&readfds);
	FD_SET(fd, &readfds);
	if (select(fd + 1, &readfds, NULL, NULL, &timeout) != 1 ||
	    !FD_ISSET(fd, &readfds))
		return fail("select-readable");
	epfd = epoll_create1(EPOLL_CLOEXEC);
	if (epfd < 0 || epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &event) < 0 ||
	    epoll_wait(epfd, &output, 1, 2000) != 1 ||
	    (output.events & EPOLLIN) == 0) {
		if (epfd >= 0)
			close(epfd);
		return fail("epoll-readable");
	}
	close(epfd);
	return 0;
}

static int blocking_stream_oracle(void)
{
	struct sockaddr_in address;
	struct sockaddr_in local;
	struct iovec send_iov[2];
	struct iovec receive_iov[2];
	struct msghdr message;
	struct sigaction action;
	socklen_t length;
	char first[2] = { 0 };
	char second[2] = { 0 };
	char reply[3] = { 0 };
	char peek[4] = { 0 };
	char drain[4096];
	int enabled;
	int listener;
	int client;
	int accepted;
	int flags;
	ssize_t sent;
	ssize_t remaining;

	listener = make_listener(&address);
	if (listener < 0)
		return fail("blocking-listener");
	client = socket(AF_INET, SOCK_STREAM, 0);
	if (client < 0 || connect(client, (struct sockaddr *)&address,
				 sizeof(address)) < 0)
		return fail("blocking-connect");
	errno = 0;
	if (expect_errno(connect(client, (struct sockaddr *)&address,
				 sizeof(address)), EISCONN,
			 "already-connected"))
		return 1;
	length = sizeof(local);
	if (getsockname(client, (struct sockaddr *)&local, &length) < 0 ||
	    local.sin_addr.s_addr != htonl(INADDR_LOOPBACK) || local.sin_port == 0)
		return fail("implicit-bind-query");
	accepted = accept(listener, NULL, NULL);
	if (accepted < 0)
		return fail("blocking-accept");

	enabled = 1;
	if (setsockopt(client, IPPROTO_TCP, TCP_NODELAY, &enabled,
		       sizeof(enabled)) < 0)
		return fail("tcp-nodelay-set");
	enabled = 0;
	length = sizeof(enabled);
	if (getsockopt(client, IPPROTO_TCP, TCP_NODELAY, &enabled, &length) < 0 ||
	    enabled != 1)
		return fail("tcp-nodelay-query");

	send_iov[0].iov_base = (void *)"ab";
	send_iov[0].iov_len = 2;
	send_iov[1].iov_base = (void *)"cd";
	send_iov[1].iov_len = 2;
	if (writev(client, send_iov, 2) != 4 || readiness_oracle(accepted))
		return fail("vector-send-readiness");
	memset(&message, 0, sizeof(message));
	message.msg_iov = send_iov;
	message.msg_iovlen = 1;
	send_iov[0].iov_base = peek;
	send_iov[0].iov_len = sizeof(peek);
	if (recvmsg(accepted, &message, MSG_PEEK) != 4 ||
	    memcmp(peek, "abcd", 4) != 0)
		return fail("recvmsg-peek");
	receive_iov[0].iov_base = first;
	receive_iov[0].iov_len = sizeof(first);
	receive_iov[1].iov_base = second;
	receive_iov[1].iov_len = sizeof(second);
	if (readv(accepted, receive_iov, 2) != 4 ||
	    memcmp(first, "ab", 2) != 0 || memcmp(second, "cd", 2) != 0)
		return fail("readv-consume");
	memset(&message, 0, sizeof(message));
	send_iov[0].iov_base = (void *)"xyz";
	send_iov[0].iov_len = 3;
	message.msg_iov = send_iov;
	message.msg_iovlen = 1;
	if (sendmsg(accepted, &message, 0) != 3 || recv(client, reply, 3, 0) != 3 ||
	    memcmp(reply, "xyz", 3) != 0)
		return fail("sendmsg-scalar-receive");

	memset(large_payload, 0x5a, sizeof(large_payload));
	flags = fcntl(client, F_GETFL);
	if (flags < 0 || fcntl(client, F_SETFL, flags | O_NONBLOCK) < 0)
		return fail("partial-set-nonblock");
	sent = send(client, large_payload, sizeof(large_payload), 0);
	if (sent <= 0 || (size_t)sent >= sizeof(large_payload))
		return fail("partial-send-prefix");
	if (shutdown(client, SHUT_WR) < 0)
		return fail("shutdown-write");
	remaining = sent;
	while (remaining > 0) {
		ssize_t received = recv(accepted, drain,
					remaining < (ssize_t)sizeof(drain) ?
					remaining : (ssize_t)sizeof(drain), 0);
		if (received <= 0)
			return fail("partial-drain");
		remaining -= received;
	}
	if (recv(accepted, drain, 1, 0) != 0 ||
	    wait_poll(accepted, POLLRDHUP, POLLRDHUP, "peer-rdhup"))
		return fail("eof-after-shutdown");

	memset(&action, 0, sizeof(action));
	action.sa_handler = handle_sigpipe;
	sigemptyset(&action.sa_mask);
	if (sigaction(SIGPIPE, &action, NULL) < 0)
		return fail("sigpipe-handler");
	sigpipe_count = 0;
	errno = 0;
	if (expect_errno(send(client, "n", 1, MSG_NOSIGNAL), EPIPE,
			 "nosignal-epipe") || sigpipe_count != 0)
		return fail("nosignal-suppression");
	errno = 0;
	if (expect_errno(send(client, "s", 1, 0), EPIPE, "sigpipe-epipe") ||
	    sigpipe_count != 1)
		return fail("sigpipe-delivery");

	close(accepted);
	close(client);
	close(listener);
	return 0;
}

static int wildcard_bind_projection_oracle(void)
{
	struct sockaddr_in address;
	struct sockaddr_in local;
	struct sockaddr_in wildcard;
	socklen_t length;
	int listener;
	int client;
	int accepted;

	listener = make_listener(&address);
	if (listener < 0)
		return fail("wildcard-listener");
	client = socket(AF_INET, SOCK_STREAM, 0);
	memset(&wildcard, 0, sizeof(wildcard));
	wildcard.sin_family = AF_INET;
	if (client < 0 || bind(client, (struct sockaddr *)&wildcard,
			       sizeof(wildcard)) < 0 ||
	    connect(client, (struct sockaddr *)&address, sizeof(address)) < 0)
		return fail("wildcard-connect");
	length = sizeof(local);
	if (getsockname(client, (struct sockaddr *)&local, &length) < 0 ||
	    local.sin_addr.s_addr != htonl(INADDR_LOOPBACK) || local.sin_port == 0)
		return fail("wildcard-connected-query");
	accepted = accept(listener, NULL, NULL);
	if (accepted < 0)
		return fail("wildcard-accept");
	close(accepted);
	close(client);
	close(listener);
	return 0;
}

static int finish_nonblocking_connect(int fd)
{
	socklen_t length = sizeof(int);
	int error = -1;

	if (wait_poll(fd, POLLOUT, POLLOUT, "nonblocking-connect-poll") ||
	    getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &length) < 0 || error != 0)
		return fail("nonblocking-connect-so-error");
	return 0;
}

static int start_nonblocking_connect(int fd, const struct sockaddr_in *address)
{
	errno = 0;
	if (expect_errno(connect(fd, (const struct sockaddr *)address,
				 sizeof(*address)), EINPROGRESS,
			 "nonblocking-connect-start"))
		return 1;
	errno = 0;
	if (expect_errno(connect(fd, (const struct sockaddr *)address,
				 sizeof(*address)), EALREADY,
			 "nonblocking-connect-repeat"))
		return 1;
	return 0;
}

static int nonblocking_accept_and_lifecycle_oracle(void)
{
	struct sockaddr_in address;
	struct sockaddr_in peer;
	socklen_t length;
	char byte = 0;
	int listener;
	int first;
	int second;
	int accepted;
	int alias;

	listener = make_listener(&address);
	if (listener < 0)
		return fail("nonblocking-listener");
	if (fcntl(listener, F_SETFL, fcntl(listener, F_GETFL) | O_NONBLOCK) < 0)
		return fail("listener-nonblock");
	errno = 0;
	if (expect_errno(accept(listener, NULL, NULL), EAGAIN,
			 "accept-empty-nonblocking"))
		return 1;

	first = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
	if (first < 0 || start_nonblocking_connect(first, &address) ||
	    finish_nonblocking_connect(first))
		return 1;
	if (wait_poll(listener, POLLIN, POLLIN, "first-child-readable"))
		return 1;
	length = sizeof(peer);
	errno = 0;
	if (expect_errno(syscall(SYS_accept4, listener, (void *)1, &length, 0),
			 EFAULT, "accept-address-copy-rollback"))
		return 1;
	close(first);

	second = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
	if (second < 0 || start_nonblocking_connect(second, &address) ||
	    finish_nonblocking_connect(second))
		return 1;
	if (wait_poll(listener, POLLIN, POLLIN, "second-child-readable"))
		return 1;
	length = sizeof(peer);
	accepted = accept4(listener, (struct sockaddr *)&peer, &length,
			   SOCK_NONBLOCK | SOCK_CLOEXEC);
	if (accepted < 0 || peer.sin_family != AF_INET ||
	    (fcntl(accepted, F_GETFL) & O_NONBLOCK) == 0 ||
	    (fcntl(accepted, F_GETFD) & FD_CLOEXEC) == 0)
		return fail("accept4-flags-peer");
	alias = dup(accepted);
	if (alias < 0 || (fcntl(alias, F_GETFD) & FD_CLOEXEC) != 0)
		return fail("dup-clears-cloexec");
	close(accepted);
	if (send(second, "d", 1, 0) != 1 ||
	    wait_poll(alias, POLLIN, POLLIN, "dup-nonfinal-close") ||
	    recv(alias, &byte, 1, 0) != 1 || byte != 'd')
		return fail("dup-after-nonfinal-close");
	close(alias);
	if (wait_poll(second, POLLIN | POLLRDHUP, POLLIN | POLLRDHUP,
		      "final-close-hint") || recv(second, &byte, 1, 0) != 0)
		return fail("final-close-eof");
	close(second);
	close(listener);
	return 0;
}

static int refused_so_error_oracle(void)
{
	struct sockaddr_in address;
	struct sockaddr_in local;
	socklen_t length = sizeof(int);
	socklen_t local_length;
	int client;
	int error = 0;

	memset(&address, 0, sizeof(address));
	address.sin_family = AF_INET;
	address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
	address.sin_port = htons(25999);
	client = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
	if (client < 0)
		return fail("refused-client");
	errno = 0;
	if (expect_errno(connect(client, (struct sockaddr *)&address,
				 sizeof(address)), EINPROGRESS,
			 "refused-connect-start") ||
	    wait_poll(client, POLLOUT | POLLERR, POLLOUT | POLLERR,
		      "refused-connect-poll"))
		return 1;
	if (getsockopt(client, SOL_SOCKET, SO_ERROR, &error, &length) < 0 ||
	    error != ECONNREFUSED)
		return fail("refused-so-error");
	error = -1;
	length = sizeof(error);
	if (getsockopt(client, SOL_SOCKET, SO_ERROR, &error, &length) < 0 ||
	    error != 0)
		return fail("refused-so-error-consumed");
	errno = 0;
	if (expect_errno(connect(client, (struct sockaddr *)&address,
				 sizeof(address)), ECONNABORTED,
			 "refused-connect-after-consume"))
		return 1;
	local_length = sizeof(local);
	if (getsockname(client, (struct sockaddr *)&local, &local_length) < 0 ||
	    local.sin_addr.s_addr != htonl(INADDR_ANY) || local.sin_port == 0)
		return fail("refused-retained-wildcard-bind");
	errno = 0;
	if (expect_errno(connect(client, (struct sockaddr *)&address,
				 sizeof(address)), EINPROGRESS,
			 "refused-connect-retry"))
		return 1;
	close(client);
	return 0;
}

static int blocking_accept_wait_oracle(void)
{
	struct sockaddr_in address;
	struct timespec delay = { .tv_sec = 0, .tv_nsec = 20 * 1000 * 1000 };
	char byte = 0;
	int listener;
	int accepted;
	int status;
	pid_t child;

	listener = make_listener(&address);
	if (listener < 0)
		return fail("wait-listener");
	child = fork();
	if (child < 0)
		return fail("wait-fork");
	if (child == 0) {
		int client;

		close(listener);
		nanosleep(&delay, NULL);
		client = socket(AF_INET, SOCK_STREAM, 0);
		if (client < 0 || connect(client, (struct sockaddr *)&address,
					  sizeof(address)) < 0 ||
		    send(client, "w", 1, 0) != 1)
			_exit(1);
		close(client);
		_exit(0);
	}
	accepted = accept(listener, NULL, NULL);
	if (accepted < 0 || recv(accepted, &byte, 1, 0) != 1 || byte != 'w')
		return fail("blocking-accept-wait");
	close(accepted);
	close(listener);
	if (waitpid(child, &status, 0) != child || !WIFEXITED(status) ||
	    WEXITSTATUS(status) != 0)
		return fail("blocking-accept-child");
	return 0;
}

int main(void)
{
	if (creation_and_rejection_oracle() || blocking_stream_oracle() ||
	    nonblocking_accept_and_lifecycle_oracle() ||
	    wildcard_bind_projection_oracle() || refused_so_error_oracle() ||
	    blocking_accept_wait_oracle())
		return 1;
	puts("TPASS: tcp_r0_oracle");
	return 0;
}
