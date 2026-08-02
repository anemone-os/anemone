#define _GNU_SOURCE

/*
 * Focused dual-libc oracle for the Socket R1 ABI and readiness cutover.
 *
 * This source is intentionally independent of the normal LTP build so the
 * same raw-syscall assertions can be statically linked against each guest's
 * glibc and musl toolchain.  It is a validation asset, not a production
 * dependency; the Stage 4 closure records the exact compiler and binary
 * identities used for the cutover run.
 */

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static const struct timespec zero_timeout = { 0, 0 };
static const char *socket_path = "/tmp/anemone-socket-r1-oracle";

static int fail(const char *step)
{
	fprintf(stderr, "TFAIL: socket_r1_oracle step=%s errno=%d\n", step, errno);
	return 1;
}

static int expect_errno(long result, int expected, const char *step)
{
	if (result == -1 && errno == expected)
		return 0;
	fprintf(stderr,
		"TFAIL: socket_r1_oracle step=%s result=%ld errno=%d expected=%d\n",
		step, result, errno, expected);
	return 1;
}

static int expect_mask(short actual, short required, short forbidden,
		       const char *step)
{
	if ((actual & required) == required && (actual & forbidden) == 0)
		return 0;
	fprintf(stderr,
		"TFAIL: socket_r1_oracle step=%s actual=0x%x required=0x%x forbidden=0x%x\n",
		step, (unsigned short)actual, (unsigned short)required,
		(unsigned short)forbidden);
	return 1;
}

static long raw_shutdown(int fd, int how)
{
	errno = 0;
	return syscall(SYS_shutdown, fd, how);
}

static int shutdown_lookup_and_role_oracle(void)
{
	struct sockaddr_un address = { .sun_family = AF_UNIX };
	socklen_t address_len;
	char byte = 'r';
	int pipefd[2];
	int listener;
	int client;
	int accepted;
	int how;

	if (pipe(pipefd) < 0)
		return fail("shutdown-pipe");
	if (expect_errno(raw_shutdown(-1, -1), EBADF,
			 "shutdown-invalid-fd-priority") ||
	    expect_errno(raw_shutdown(pipefd[0], SHUT_RDWR), ENOTSOCK,
			 "shutdown-nonsocket-valid-how") ||
	    expect_errno(raw_shutdown(pipefd[0], -1), ENOTSOCK,
			 "shutdown-nonsocket-invalid-how"))
		return 1;
	close(pipefd[0]);
	close(pipefd[1]);

	if (strlen(socket_path) >= sizeof(address.sun_path))
		return fail("shutdown-path-size");
	strcpy(address.sun_path, socket_path);
	address_len = offsetof(struct sockaddr_un, sun_path) +
		      strlen(socket_path) + 1;
	unlink(socket_path);

	listener = socket(AF_UNIX, SOCK_STREAM, 0);
	client = socket(AF_UNIX, SOCK_STREAM, 0);
	if (listener < 0 || client < 0)
		return fail("shutdown-role-sockets");
	if (expect_errno(raw_shutdown(client, -1), EINVAL,
			 "shutdown-socket-invalid-how"))
		return 1;
	for (how = SHUT_RD; how <= SHUT_RDWR; how++) {
		if (expect_errno(raw_shutdown(client, how), ENOTCONN,
				 "shutdown-unconnected-role"))
			return 1;
	}

	if (bind(listener, (struct sockaddr *)&address, address_len) < 0)
		return fail("shutdown-role-bind");
	for (how = SHUT_RD; how <= SHUT_RDWR; how++) {
		if (expect_errno(raw_shutdown(listener, how), ENOTCONN,
				 "shutdown-bound-role"))
			return 1;
	}
	if (listen(listener, 1) < 0)
		return fail("shutdown-role-listen");
	for (how = SHUT_RD; how <= SHUT_RDWR; how++) {
		if (expect_errno(raw_shutdown(listener, how), ENOTCONN,
				 "shutdown-listening-role"))
			return 1;
	}

	if (connect(client, (struct sockaddr *)&address, address_len) < 0)
		return fail("shutdown-role-connect-after-rejection");
	accepted = accept(listener, NULL, NULL);
	if (accepted < 0 || write(client, &byte, 1) != 1)
		return fail("shutdown-role-accept-after-rejection");
	byte = 0;
	if (read(accepted, &byte, 1) != 1 || byte != 'r')
		return fail("shutdown-role-data-after-rejection");

	close(accepted);
	close(client);
	close(listener);
	unlink(socket_path);
	return 0;
}

static int address_option_and_copy_oracle(void)
{
	struct sockaddr_un address = { .sun_family = AF_UNIX };
	unsigned char output[sizeof(struct sockaddr_un)];
	socklen_t address_len;
	socklen_t short_len;
	socklen_t option_len;
	char byte = 'c';
	int option;
	int pair[2];
	int listener;
	int client;
	int accepted;

	strcpy(address.sun_path, socket_path);
	address_len = offsetof(struct sockaddr_un, sun_path) +
		      strlen(socket_path) + 1;
	unlink(socket_path);
	listener = socket(AF_UNIX, SOCK_STREAM, 0);
	client = socket(AF_UNIX, SOCK_STREAM, 0);
	if (listener < 0 || client < 0 ||
	    bind(listener, (struct sockaddr *)&address, address_len) < 0 ||
	    listen(listener, 1) < 0 ||
	    connect(client, (struct sockaddr *)&address, address_len) < 0)
		return fail("address-setup");
	accepted = accept(listener, NULL, NULL);
	if (accepted < 0)
		return fail("address-accept");

	memset(output, 0xa5, sizeof(output));
	short_len = 1;
	if (syscall(SYS_getsockname, listener, output, &short_len) != 0 ||
	    short_len != address_len || output[0] != (unsigned char)AF_UNIX)
		return fail("getsockname-short-prefix-actual-len");
	short_len = 1;
	if (syscall(SYS_getpeername, accepted, output, &short_len) != 0 ||
	    short_len != sizeof(sa_family_t) ||
	    output[0] != (unsigned char)AF_UNIX)
		return fail("getpeername-short-prefix-actual-len");
	short_len = address_len;
	errno = 0;
	if (expect_errno(syscall(SYS_getsockname, listener, (void *)1, &short_len),
			 EFAULT, "getsockname-address-copy-fault"))
		return 1;
	errno = 0;
	if (expect_errno(syscall(SYS_getsockname, listener, output, (void *)1),
			 EFAULT, "getsockname-addrlen-copy-fault"))
		return 1;

	option = -1;
	option_len = 1;
	if (syscall(SYS_getsockopt, client, SOL_SOCKET, SO_TYPE,
		    &option, &option_len) != 0 ||
	    option_len != 1 || ((unsigned char *)&option)[0] != SOCK_STREAM)
		return fail("getsockopt-short-prefix");
	option_len = sizeof(option);
	errno = 0;
	if (expect_errno(syscall(SYS_getsockopt, client, SOL_SOCKET, SO_ERROR,
				 &option, &option_len),
			 ENOPROTOOPT, "getsockopt-so-error-unsupported"))
		return 1;
	option_len = (socklen_t)-1;
	errno = 0;
	if (expect_errno(syscall(SYS_getsockopt, client, SOL_SOCKET, SO_TYPE,
				 &option, &option_len),
			 EINVAL, "getsockopt-negative-length"))
		return 1;
	errno = 0;
	if (expect_errno(syscall(SYS_setsockopt, -1, SOL_SOCKET, SO_TYPE,
				 &option, -1),
			 EINVAL, "setsockopt-negative-length-priority"))
		return 1;
	option_len = sizeof(option);
	errno = 0;
	if (expect_errno(syscall(SYS_setsockopt, client, SOL_SOCKET, SO_TYPE,
				 &option, option_len),
			 ENOPROTOOPT, "setsockopt-unsupported"))
		return 1;

	if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) < 0 ||
	    write(pair[1], &byte, 1) != 1)
		return fail("recvfrom-copy-setup");
	errno = 0;
	if (expect_errno(syscall(SYS_recvfrom, pair[0], (void *)1, 1, 0,
				 NULL, NULL),
			 EFAULT, "recvfrom-copy-fault"))
		return 1;
	byte = 0;
	if (recvfrom(pair[0], &byte, 1, 0, NULL, NULL) != 1 || byte != 'c')
		return fail("recvfrom-copy-fault-retains-prefix");

	close(pair[0]);
	close(pair[1]);
	close(accepted);
	close(client);
	close(listener);
	unlink(socket_path);
	return 0;
}

static int poll_now(int fd, short interest, short *result)
{
	struct pollfd pfd = { .fd = fd, .events = interest };
	int ready = ppoll(&pfd, 1, &zero_timeout, NULL);

	if (ready < 0)
		return -1;
	*result = pfd.revents;
	return ready;
}

static int listener_readiness_oracle(void)
{
	struct sockaddr_un address = { .sun_family = AF_UNIX };
	socklen_t address_len;
	short events;
	int listener;
	int client;
	int accepted;

	strcpy(address.sun_path, socket_path);
	address_len = offsetof(struct sockaddr_un, sun_path) +
		      strlen(socket_path) + 1;
	unlink(socket_path);
	listener = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0);
	if (listener < 0 ||
	    bind(listener, (struct sockaddr *)&address, address_len) < 0)
		return fail("readiness-listener-setup");
	if (poll_now(listener, POLLIN | POLLOUT | POLLRDHUP, &events) != 1 ||
	    expect_mask(events, POLLOUT | POLLHUP, POLLIN | POLLRDHUP,
			"readiness-bound-role"))
		return 1;
	if (listen(listener, 1) < 0 ||
	    poll_now(listener, POLLIN | POLLOUT | POLLRDHUP, &events) != 0 ||
	    events != 0)
		return fail("readiness-listener-empty");
	client = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0);
	if (client < 0 ||
	    connect(client, (struct sockaddr *)&address, address_len) < 0)
		return fail("readiness-listener-connect");
	if (poll_now(listener, POLLIN | POLLOUT | POLLRDHUP, &events) != 1 ||
	    events != POLLIN)
		return fail("readiness-listener-admitted");
	accepted = accept(listener, NULL, NULL);
	if (accepted < 0 ||
	    poll_now(listener, POLLIN | POLLOUT | POLLRDHUP, &events) != 0 ||
	    events != 0)
		return fail("readiness-listener-consumed");
	close(accepted);
	close(client);
	close(listener);
	unlink(socket_path);
	return 0;
}

static int half_close_readiness_oracle(void)
{
	struct epoll_event interest = { 0 };
	struct epoll_event event = { 0 };
	fd_set readfds;
	fd_set writefds;
	struct timespec timeout = zero_timeout;
	char byte;
	short events;
	int pair[2];
	int epfd;
	int ready;

	if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) < 0 ||
	    write(pair[1], "x", 1) != 1 || shutdown(pair[1], SHUT_WR) < 0)
		return fail("readiness-buffered-shutdown");
	if (poll_now(pair[0], POLLIN, &events) != 1 || events != POLLIN)
		return fail("readiness-unrequested-rdhup");
	if (poll_now(pair[0], POLLIN | POLLOUT | POLLRDHUP, &events) != 1 ||
	    expect_mask(events, POLLIN | POLLOUT | POLLRDHUP, POLLHUP,
			"readiness-requested-rdhup"))
		return 1;
	if (read(pair[0], &byte, 1) != 1 || byte != 'x' ||
	    read(pair[0], &byte, 1) != 0)
		return fail("readiness-buffer-before-eof");

	FD_ZERO(&readfds);
	FD_ZERO(&writefds);
	FD_SET(pair[0], &readfds);
	FD_SET(pair[0], &writefds);
	ready = pselect(pair[0] + 1, &readfds, &writefds, NULL, &timeout, NULL);
	if (ready != 2 || !FD_ISSET(pair[0], &readfds) ||
	    !FD_ISSET(pair[0], &writefds))
		return fail("readiness-pselect-half-close");

	epfd = epoll_create1(0);
	if (epfd < 0)
		return fail("readiness-epoll-create");
	interest.events = EPOLLRDHUP;
	interest.data.u64 = 0x5231;
	if (epoll_ctl(epfd, EPOLL_CTL_ADD, pair[0], &interest) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 ||
	    event.events != EPOLLRDHUP || event.data.u64 != 0x5231 ||
	    epoll_wait(epfd, &event, 1, 0) != 1)
		return fail("readiness-epoll-rdhup-level");
	interest.events = EPOLLRDHUP | EPOLLET;
	if (epoll_ctl(epfd, EPOLL_CTL_MOD, pair[0], &interest) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 ||
	    event.events != EPOLLRDHUP || epoll_wait(epfd, &event, 1, 0) != 0)
		return fail("readiness-epoll-rdhup-edge");
	interest.events = EPOLLRDHUP | EPOLLONESHOT;
	if (epoll_ctl(epfd, EPOLL_CTL_MOD, pair[0], &interest) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 ||
	    event.events != EPOLLRDHUP || epoll_wait(epfd, &event, 1, 0) != 0 ||
	    epoll_ctl(epfd, EPOLL_CTL_MOD, pair[0], &interest) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 || event.events != EPOLLRDHUP)
		return fail("readiness-epoll-rdhup-oneshot");
	if (shutdown(pair[0], SHUT_WR) < 0)
		return fail("readiness-local-shutdown-write");
	interest.events = 0;
	if (epoll_ctl(epfd, EPOLL_CTL_MOD, pair[0], &interest) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 || event.events != EPOLLHUP)
		return fail("readiness-mandatory-full-hup");

	close(epfd);
	close(pair[0]);
	close(pair[1]);
	return 0;
}

int main(void)
{
	if (shutdown_lookup_and_role_oracle() ||
	    address_option_and_copy_oracle() ||
	    listener_readiness_oracle() ||
	    half_close_readiness_oracle())
		return 1;
	puts("TPASS: socket_r1_oracle abi shutdown addrlen copyfault listener ppoll pselect rdhup hup lt et oneshot");
	return 0;
}
