#define _GNU_SOURCE

/*
 * Production-path IPv4 UDP extended-error oracle. The same source is used
 * for host Linux characterization and both static RV64 guest libc builds.
 */

#include <errno.h>
#include <limits.h>
#include <stddef.h>
#include <time.h>
#include <linux/errqueue.h>
#include <netinet/in.h>
#include <poll.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/utsname.h>
#include <unistd.h>

static int fail(const char *step)
{
	fprintf(stderr, "TFAIL: udp_errqueue step=%s errno=%d\n", step, errno);
	return 1;
}

static int closed_loopback(struct sockaddr_in *address)
{
	socklen_t len = sizeof(*address);
	int fd = socket(AF_INET, SOCK_DGRAM, 0);

	memset(address, 0, sizeof(*address));
	address->sin_family = AF_INET;
	address->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
	if (fd < 0 || bind(fd, (struct sockaddr *)address, sizeof(*address)) < 0 ||
	    getsockname(fd, (struct sockaddr *)address, &len) < 0) {
		if (fd >= 0)
			close(fd);
		return -1;
	}
	close(fd);
	return 0;
}

static int enable_errors(int fd, int enabled)
{
	int observed = -1;
	socklen_t len = sizeof(observed);

	if (setsockopt(fd, SOL_IP, IP_RECVERR, &enabled, sizeof(enabled)) < 0 ||
	    getsockopt(fd, SOL_IP, IP_RECVERR, &observed, &len) < 0 ||
	    len != sizeof(observed) || observed != !!enabled)
		return -1;
	return 0;
}

static int wait_error(int fd)
{
	/* POLLERR is mandatory and must wake even with no requested event bits. */
	struct pollfd pfd = { .fd = fd, .events = 0 };
	int ready = poll(&pfd, 1, 2000);

	if (ready <= 0 || !(pfd.revents & POLLERR))
		return -1;
	return 0;
}

static int take_so_error(int fd, int expected)
{
	int value = -1;
	socklen_t len = sizeof(value);

	if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &value, &len) < 0 ||
	    len != sizeof(value) || value != expected)
		return -1;
	return 0;
}

static int wait_so_error(int fd, int expected)
{
	int value;
	socklen_t len;
	int attempt;

	for (attempt = 0; attempt < 20000; attempt++) {
		value = -1;
		len = sizeof(value);
		if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &value, &len) < 0 ||
		    len != sizeof(value))
			return -1;
		if (value == expected)
			return 0;
		if (value != 0)
			return -1;
		sched_yield();
	}
	errno = EAGAIN;
	return -1;
}

static int receive_error(int fd, const char *marker,
			 const struct sockaddr_in *expected_destination,
			 size_t payload_capacity, size_t control_len,
			 int expected_extra_flags)
{
	struct sockaddr_in name;
	unsigned char control[128];
	char payload[64];
	struct iovec iov = { .iov_base = payload, .iov_len = payload_capacity };
	struct msghdr message = {
		.msg_name = &name,
		.msg_namelen = sizeof(name),
		.msg_iov = &iov,
		.msg_iovlen = 1,
		.msg_control = control,
		.msg_controllen = control_len,
	};
	struct cmsghdr *cmsg;
	struct sock_extended_err *error;
	struct sockaddr_in *offender;
	size_t expected_length = strlen(marker) < payload_capacity ?
		strlen(marker) : payload_capacity;
	ssize_t received;

	memset(&name, 0, sizeof(name));
	memset(control, 0, sizeof(control));
	errno = 0;
	received = recvmsg(fd, &message, MSG_ERRQUEUE | MSG_DONTWAIT);
	if (received < 0 || (size_t)received != expected_length ||
	    memcmp(payload, marker, expected_length) != 0 ||
	    name.sin_family != AF_INET ||
	    name.sin_addr.s_addr != expected_destination->sin_addr.s_addr ||
	    name.sin_port != expected_destination->sin_port ||
	    !(message.msg_flags & MSG_ERRQUEUE) ||
	    (message.msg_flags & expected_extra_flags) != expected_extra_flags)
		return -1;
	if (expected_extra_flags & MSG_CTRUNC)
		return 0;
	cmsg = CMSG_FIRSTHDR(&message);
	if (!cmsg || cmsg->cmsg_level != SOL_IP || cmsg->cmsg_type != IP_RECVERR ||
	    cmsg->cmsg_len != CMSG_LEN(sizeof(*error) + sizeof(*offender)))
		return -1;
	error = (struct sock_extended_err *)CMSG_DATA(cmsg);
	offender = (struct sockaddr_in *)(error + 1);
	if (error->ee_errno != ECONNREFUSED || error->ee_origin != SO_EE_ORIGIN_ICMP ||
	    error->ee_type != 3 || error->ee_code != 3 || error->ee_pad != 0 ||
	    error->ee_info != 0 || error->ee_data != 0 ||
	    offender->sin_family != AF_INET || offender->sin_port != 0 ||
	    offender->sin_addr.s_addr != htonl(INADDR_LOOPBACK))
		return -1;
	return 0;
}

static int send_wait_take(int fd, const char *marker, int take_pending)
{
	if (send(fd, marker, strlen(marker), 0) != (ssize_t)strlen(marker) ||
	    wait_error(fd) < 0)
		return -1;
	if (take_pending && wait_so_error(fd, ECONNREFUSED) < 0)
		return -1;
	return 0;
}

static int header_copy_fault_consumes_record(int fd, const char *marker)
{
	long page_size = sysconf(_SC_PAGESIZE);
	void *mapping;
	struct msghdr *message;
	struct iovec iov;
	char payload[64];
	int saved_errno;

	if (page_size <= 0)
		return -1;
	mapping = mmap(NULL, (size_t)page_size, PROT_READ | PROT_WRITE,
		       MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (mapping == MAP_FAILED)
		return -1;
	message = mapping;
	iov.iov_base = payload;
	iov.iov_len = sizeof(payload);
	memset(message, 0, sizeof(*message));
	message->msg_iov = &iov;
	message->msg_iovlen = 1;
	if (mprotect(mapping, (size_t)page_size, PROT_READ) < 0) {
		munmap(mapping, (size_t)page_size);
		return -1;
	}
	errno = 0;
	if (syscall(SYS_recvmsg, fd, message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 ||
	    errno != EFAULT) {
		mprotect(mapping, (size_t)page_size, PROT_READ | PROT_WRITE);
		munmap(mapping, (size_t)page_size);
		return -1;
	}
	saved_errno = errno;
	if (mprotect(mapping, (size_t)page_size, PROT_READ | PROT_WRITE) < 0) {
		munmap(mapping, (size_t)page_size);
		return -1;
	}
	memset(message, 0, sizeof(*message));
	errno = 0;
	if (recvmsg(fd, message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 ||
	    errno != EAGAIN) {
		munmap(mapping, (size_t)page_size);
		return -1;
	}
	munmap(mapping, (size_t)page_size);
	(void)marker;
	errno = saved_errno;
	return 0;
}

static int connected_oracle(void)
{
	const char *first = "udp-errqueue-one";
	const char *second = "udp-errqueue-two";
	const char *third = "udp-errqueue-three";
	struct sockaddr_in closed;
	struct epoll_event interest = { .events = EPOLLIN };
	struct epoll_event event;
	char control[128];
	char payload[8];
	struct iovec iov;
	struct msghdr message;
	int fd, epfd;

	if (closed_loopback(&closed) < 0)
		return fail("closed-port");
	fd = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	if (fd < 0 || enable_errors(fd, 1) < 0 ||
	    connect(fd, (struct sockaddr *)&closed, sizeof(closed)) < 0)
		return fail("connected-setup");
	if (send_wait_take(fd, first, 1) < 0 || take_so_error(fd, 0) < 0)
		return fail("pending-consume-once");

	epfd = epoll_create1(0);
	if (epfd < 0 || epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &interest) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 || !(event.events & EPOLLERR))
		return fail("epoll-mandatory-error");
	if (receive_error(fd, first, &closed, 64, sizeof(control), 0) < 0)
		return fail("first-errqueue-projection");
	errno = 0;
	memset(&message, 0, sizeof(message));
	if (recvmsg(fd, &message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 || errno != EAGAIN)
		return fail("empty-errqueue");
	if (epoll_wait(epfd, &event, 1, 0) != 0 ||
	    send_wait_take(fd, second, 1) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 1 || !(event.events & EPOLLERR) ||
	    receive_error(fd, second, &closed, 64, sizeof(control), 0) < 0 ||
	    epoll_wait(epfd, &event, 1, 0) != 0)
		return fail("epoll-error-rearm");
	close(epfd);

	if (send_wait_take(fd, second, 1) < 0 ||
	    send_wait_take(fd, third, 1) < 0 ||
	    receive_error(fd, second, &closed, 64, sizeof(control), 0) < 0 ||
	    receive_error(fd, third, &closed, 64, sizeof(control), 0) < 0)
		return fail("fifo-order");

	if (send_wait_take(fd, first, 1) < 0 ||
	    receive_error(fd, first, &closed, 64,
			  sizeof(struct cmsghdr) + 4, MSG_CTRUNC) < 0)
		return fail("control-truncation");
	if (send_wait_take(fd, first, 1) < 0 ||
	    receive_error(fd, first, &closed, 4, sizeof(control), MSG_TRUNC) < 0)
		return fail("payload-truncation");

	if (send_wait_take(fd, second, 0) < 0) {
		return fail("ordinary-send-error-setup");
	}
	errno = 0;
	if (send(fd, third, strlen(third), 0) != -1 || errno != ECONNREFUSED ||
	    take_so_error(fd, 0) < 0 ||
	    receive_error(fd, second, &closed, 64, sizeof(control), 0) < 0)
		return fail("ordinary-send-consumes-pending-only");

	if (send_wait_take(fd, second, 1) < 0)
		return fail("fault-setup");
	iov.iov_base = (void *)1;
	iov.iov_len = strlen(second);
	memset(&message, 0, sizeof(message));
	message.msg_iov = &iov;
	message.msg_iovlen = 1;
	message.msg_control = control;
	message.msg_controllen = sizeof(control);
	errno = 0;
	if (recvmsg(fd, &message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 || errno != EFAULT)
		return fail("copy-fault");
	iov.iov_base = payload;
	iov.iov_len = sizeof(payload);
	errno = 0;
	if (recvmsg(fd, &message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 || errno != EAGAIN)
		return fail("fault-consumes-record");
	if (send_wait_take(fd, third, 1) < 0 ||
	    header_copy_fault_consumes_record(fd, third) < 0)
		return fail("header-copy-fault-consumes-record");

	if (send_wait_take(fd, third, 0) < 0 || enable_errors(fd, 0) < 0 ||
	    take_so_error(fd, ECONNREFUSED) < 0)
		return fail("disable-preserves-pending");
	errno = 0;
	memset(&message, 0, sizeof(message));
	if (recvmsg(fd, &message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 || errno != EAGAIN)
		return fail("disable-purges-fifo");
	close(fd);
	return 0;
}

static int close_reuse_clears_endpoint_error_state(void)
{
	const char *marker = "close-reuse";
	struct sockaddr_in local = {
		.sin_family = AF_INET,
		.sin_addr.s_addr = htonl(INADDR_LOOPBACK),
	};
	struct sockaddr_in closed;
	struct msghdr message;
	socklen_t local_len = sizeof(local);
	int old_fd = -1, replacement = -1;

	if (closed_loopback(&closed) < 0)
		return fail("reuse-closed-port");
	old_fd = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	if (old_fd < 0 || bind(old_fd, (struct sockaddr *)&local, sizeof(local)) < 0 ||
	    getsockname(old_fd, (struct sockaddr *)&local, &local_len) < 0 ||
	    enable_errors(old_fd, 1) < 0 ||
	    connect(old_fd, (struct sockaddr *)&closed, sizeof(closed)) < 0 ||
	    send_wait_take(old_fd, marker, 0) < 0)
		return fail("reuse-old-endpoint");
	close(old_fd);
	replacement = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	if (replacement < 0 ||
	    bind(replacement, (struct sockaddr *)&local, sizeof(local)) < 0 ||
	    enable_errors(replacement, 1) < 0 || take_so_error(replacement, 0) < 0)
		return fail("reuse-new-endpoint");
	memset(&message, 0, sizeof(message));
	errno = 0;
	if (recvmsg(replacement, &message, MSG_ERRQUEUE | MSG_DONTWAIT) != -1 ||
	    errno != EAGAIN)
		return fail("reuse-no-stale-error");
	close(replacement);
	return 0;
}

static int unconnected_oracle(void)
{
	const char *marker = "udp-unconnected";
	struct sockaddr_in closed;
	char control[128];
	int fd;

	if (closed_loopback(&closed) < 0)
		return fail("unconnected-closed-port");
	fd = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	if (fd < 0 || enable_errors(fd, 1) < 0 ||
	    sendto(fd, marker, strlen(marker), 0,
		   (struct sockaddr *)&closed, sizeof(closed)) != (ssize_t)strlen(marker) ||
	    wait_error(fd) < 0 || take_so_error(fd, ECONNREFUSED) < 0 ||
	    receive_error(fd, marker, &closed, 64, sizeof(control), 0) < 0)
		return fail("unconnected-full-chain");
	close(fd);
	return 0;
}

static int receive_datagram(int fd, const char *expected)
{
	struct pollfd pfd = { .fd = fd, .events = POLLIN };
	char payload[64];
	ssize_t received;

	if (poll(&pfd, 1, 2000) != 1 || !(pfd.revents & POLLIN))
		return -1;
	received = recv(fd, payload, sizeof(payload), 0);
	if (received != (ssize_t)strlen(expected) ||
	    memcmp(payload, expected, strlen(expected)) != 0)
		return -1;
	return 0;
}

static int sendmmsg_oracle(void)
{
	const char *payloads[] = { "sendmmsg-one", "sendmmsg-two" };
	const char *partial = "sendmmsg-partial";
	struct sockaddr_in address = {
		.sin_family = AF_INET,
		.sin_addr.s_addr = htonl(INADDR_LOOPBACK),
	};
	struct iovec iov[2];
	struct mmsghdr messages[2];
	socklen_t address_len = sizeof(address);
	char trailing;
	int receiver, sender;

	receiver = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	sender = socket(AF_INET, SOCK_DGRAM, 0);
	if (receiver < 0 || sender < 0 ||
	    bind(receiver, (struct sockaddr *)&address, sizeof(address)) < 0 ||
	    getsockname(receiver, (struct sockaddr *)&address, &address_len) < 0 ||
	    connect(sender, (struct sockaddr *)&address, sizeof(address)) < 0)
		return fail("sendmmsg-setup");

	memset(messages, 0, sizeof(messages));
	for (size_t index = 0; index < 2; index++) {
		iov[index].iov_base = (void *)payloads[index];
		iov[index].iov_len = strlen(payloads[index]);
		messages[index].msg_hdr.msg_iov = &iov[index];
		messages[index].msg_hdr.msg_iovlen = 1;
	}
	if (sendmmsg(sender, messages, 2, 0) != 2 ||
	    messages[0].msg_len != strlen(payloads[0]) ||
	    messages[1].msg_len != strlen(payloads[1]) ||
	    receive_datagram(receiver, payloads[0]) < 0 ||
	    receive_datagram(receiver, payloads[1]) < 0)
		return fail("sendmmsg-order-and-length");

	memset(messages, 0, sizeof(messages));
	iov[0].iov_base = (void *)partial;
	iov[0].iov_len = strlen(partial);
	iov[1].iov_base = (void *)1;
	iov[1].iov_len = 1;
	for (size_t index = 0; index < 2; index++) {
		messages[index].msg_hdr.msg_iov = &iov[index];
		messages[index].msg_hdr.msg_iovlen = 1;
		messages[index].msg_len = UINT_MAX;
	}
	if (sendmmsg(sender, messages, 2, 0) != 1 ||
	    messages[0].msg_len != strlen(partial) ||
	    messages[1].msg_len != UINT_MAX ||
	    receive_datagram(receiver, partial) < 0)
		return fail("sendmmsg-partial-success");
	errno = 0;
	if (recv(receiver, &trailing, sizeof(trailing), 0) != -1 || errno != EAGAIN)
		return fail("sendmmsg-partial-commit");

	memset(messages, 0, sizeof(messages));
	iov[0].iov_base = (void *)1;
	iov[0].iov_len = 1;
	iov[1].iov_base = (void *)payloads[1];
	iov[1].iov_len = strlen(payloads[1]);
	for (size_t index = 0; index < 2; index++) {
		messages[index].msg_hdr.msg_iov = &iov[index];
		messages[index].msg_hdr.msg_iovlen = 1;
		messages[index].msg_len = UINT_MAX;
	}
	errno = 0;
	if (sendmmsg(sender, messages, 2, 0) != -1 || errno != EFAULT ||
	    messages[0].msg_len != UINT_MAX || messages[1].msg_len != UINT_MAX)
		return fail("sendmmsg-first-failure");
	errno = 0;
	if (recv(receiver, &trailing, sizeof(trailing), 0) != -1 || errno != EAGAIN)
		return fail("sendmmsg-first-failure-no-commit");

	{
		const char *copyout = "sendmmsg-copyout";
		long page_size = sysconf(_SC_PAGESIZE);
		void *mapping;
		struct mmsghdr *faulting;
		struct iovec fault_iov = {
			.iov_base = (void *)copyout,
			.iov_len = strlen(copyout),
		};

		if (page_size <= 0)
			return fail("sendmmsg-copyout-pagesize");
		mapping = mmap(NULL, (size_t)page_size * 2,
			       PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS,
			       -1, 0);
		if (mapping == MAP_FAILED)
			return fail("sendmmsg-copyout-map");
		faulting = (struct mmsghdr *)((char *)mapping + page_size -
					       offsetof(struct mmsghdr, msg_len));
		memset(faulting, 0, sizeof(*faulting));
		faulting->msg_hdr.msg_iov = &fault_iov;
		faulting->msg_hdr.msg_iovlen = 1;
		if (mprotect((char *)mapping + page_size, (size_t)page_size,
			     PROT_NONE) < 0)
			return fail("sendmmsg-copyout-protect");
		errno = 0;
		if (syscall(SYS_sendmmsg, sender, faulting, 1, 0) != -1 ||
		    errno != EFAULT)
			return fail("sendmmsg-copyout-fault");
		if (mprotect((char *)mapping + page_size, (size_t)page_size,
			     PROT_READ | PROT_WRITE) < 0)
			return fail("sendmmsg-copyout-unprotect");
		munmap(mapping, (size_t)page_size * 2);
		if (receive_datagram(receiver, copyout) < 0)
			return fail("sendmmsg-copyout-after-commit");
	}
	close(sender);
	close(receiver);
	return 0;
}

static int sendmmsg_vlen_clamp_oracle(void)
{
	static char byte = 0x5a;
	static struct mmsghdr messages[1025];
	struct sockaddr_in address = {
		.sin_family = AF_INET,
		.sin_addr.s_addr = htonl(INADDR_LOOPBACK),
	};
	struct iovec iov = { .iov_base = &byte, .iov_len = 1 };
	socklen_t address_len = sizeof(address);
	int receiver, sender;
	int sent;

	receiver = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	sender = socket(AF_INET, SOCK_DGRAM, 0);
	if (receiver < 0 || sender < 0 ||
	    bind(receiver, (struct sockaddr *)&address, sizeof(address)) < 0 ||
	    getsockname(receiver, (struct sockaddr *)&address, &address_len) < 0 ||
	    connect(sender, (struct sockaddr *)&address, sizeof(address)) < 0)
		return fail("sendmmsg-vlen-setup");
	for (size_t index = 0; index < 1025; index++) {
		messages[index].msg_hdr.msg_iov = &iov;
		messages[index].msg_hdr.msg_iovlen = 1;
		messages[index].msg_len = UINT_MAX;
	}
	sent = sendmmsg(sender, messages, 1025, 0);
	close(sender);
	close(receiver);
	if (sent != 1024 || messages[1024].msg_len != UINT_MAX) {
		fprintf(stderr, "TINFO: sendmmsg-vlen sent=%d trailing-len=%u\n",
			sent, messages[1024].msg_len);
		return fail("sendmmsg-vlen-clamp");
	}
	for (size_t index = 0; index < 1024; index++) {
		if (messages[index].msg_len != 1)
			return fail("sendmmsg-vlen-lengths");
	}
	return 0;
}

static int sendmmsg_partial_stream_oracle(void)
{
	static const char second[] = "must-not-send";
	char fill[4096];
	char drain[4096];
	char *large;
	struct sockaddr_in address = {
		.sin_family = AF_INET,
		.sin_addr.s_addr = htonl(INADDR_LOOPBACK),
	};
	struct iovec iov[2];
	struct mmsghdr messages[2];
	socklen_t address_len = sizeof(address);
	int pair[2];
	int listener;
	int result = -1;

	listener = socket(AF_INET, SOCK_STREAM, 0);
	pair[0] = socket(AF_INET, SOCK_STREAM, 0);
	if (listener < 0 || pair[0] < 0 ||
	    bind(listener, (struct sockaddr *)&address, sizeof(address)) < 0 ||
	    getsockname(listener, (struct sockaddr *)&address, &address_len) < 0 ||
	    listen(listener, 1) < 0 ||
	    connect(pair[0], (struct sockaddr *)&address, sizeof(address)) < 0 ||
	    (pair[1] = accept(listener, NULL, NULL)) < 0)
		return fail("sendmmsg-stream-tcp-pair");
	close(listener);
	memset(fill, 0x5a, sizeof(fill));
	for (;;) {
		ssize_t written = send(pair[0], fill, sizeof(fill), MSG_DONTWAIT);
		if (written >= 0)
			continue;
		if (errno == EAGAIN)
			break;
		return fail("sendmmsg-stream-fill");
	}
	large = malloc(1024 * 1024);
	if (!large)
		return fail("sendmmsg-stream-allocation");
	memset(large, 0xa5, 1024 * 1024);
	iov[0].iov_base = large;
	iov[0].iov_len = 1024 * 1024;
	iov[1].iov_base = (void *)second;
	iov[1].iov_len = sizeof(second) - 1;
	memset(messages, 0, sizeof(messages));
	for (int attempt = 0; attempt < 16; attempt++) {
		if (recv(pair[1], drain, sizeof(drain), 0) <= 0)
			break;
		memset(messages, 0, sizeof(messages));
		for (size_t index = 0; index < 2; index++) {
			messages[index].msg_hdr.msg_iov = &iov[index];
			messages[index].msg_hdr.msg_iovlen = 1;
			messages[index].msg_len = UINT_MAX;
		}
		errno = 0;
		result = sendmmsg(pair[0], messages, 2, MSG_DONTWAIT);
		if (result < 0 && errno == EAGAIN)
			continue;
		break;
	}
	free(large);
	close(pair[0]);
	close(pair[1]);
	if (result != 1 || messages[0].msg_len == 0 ||
	    messages[0].msg_len >= 1024 * 1024 || messages[1].msg_len != UINT_MAX)
		return fail("sendmmsg-partial-stream-stop");
	return 0;
}

int main(void)
{
	struct utsname identity;

	if (uname(&identity) == 0)
		printf("UDP-ERRQUEUE-IDENTITY:sys=%s:release=%s:machine=%s\n",
		       identity.sysname, identity.release, identity.machine);
	if (connected_oracle() || unconnected_oracle() ||
	    close_reuse_clears_endpoint_error_state() || sendmmsg_oracle() ||
	    sendmmsg_vlen_clamp_oracle() || sendmmsg_partial_stream_oracle())
		return 1;
	puts("TPASS: udp_errqueue production packet chain");
	return 0;
}
