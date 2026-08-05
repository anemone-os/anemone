#include "runner.h"

#include "memory.h"
#include "runtime.h"

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static uint64_t elapsed_nanoseconds(struct timespec begin, struct timespec end)
{
    int64_t seconds = (int64_t)end.tv_sec - (int64_t)begin.tv_sec;
    int64_t nanoseconds = (int64_t)end.tv_nsec - (int64_t)begin.tv_nsec;

    if (nanoseconds < 0) {
        nanoseconds += 1000000000;
        seconds--;
    }
    if (seconds < 0)
        bench_fail("monotonic clock moved backwards", 0);
    return (uint64_t)seconds * UINT64_C(1000000000) + (uint64_t)nanoseconds;
}

static void run_child(const struct bench_case *test, size_t repeat,
    size_t repeats)
{
    struct timespec begin, end;
    struct bench_heap_stats stats;
    size_t checksum;
    uint64_t elapsed;

    if (clock_gettime(CLOCK_MONOTONIC_RAW, &begin) != 0)
        bench_fail("clock_gettime(begin)", errno);
    checksum = test->run(test->argument);
    if (clock_gettime(CLOCK_MONOTONIC_RAW, &end) != 0)
        bench_fail("clock_gettime(end)", errno);
    elapsed = elapsed_nanoseconds(begin, end);
    stats = bench_read_heap_stats();

    if (stats.available) {
        printf("BENCH name=%s repeat=%zu/%zu elapsed_ns=%" PRIu64
            " checksum=%zu virtual_kib=%zu resident_kib=%zu "
            "private_dirty_kib=%zu status=ok\n",
            test->name, repeat, repeats, elapsed, checksum,
            stats.virtual_kib, stats.resident_kib,
            stats.private_dirty_kib);
    } else {
        printf("BENCH name=%s repeat=%zu/%zu elapsed_ns=%" PRIu64
            " checksum=%zu memory=unavailable status=ok\n",
            test->name, repeat, repeats, elapsed, checksum);
    }
    if (fflush(NULL) == EOF)
        bench_fail("flush benchmark result", errno);
    _exit(EXIT_SUCCESS);
}

bool bench_run(const struct bench_case *test, size_t repeat, size_t repeats)
{
    pid_t child, waited;
    int status;

    if (fflush(NULL) == EOF)
        bench_fail("flush before fork", errno);
    child = fork();
    if (child < 0)
        bench_fail("fork", errno);
    if (!child)
        run_child(test, repeat, repeats);

    do {
        waited = waitpid(child, &status, 0);
    } while (waited < 0 && errno == EINTR);
    if (waited != child)
        bench_fail("waitpid", errno ? errno : ECHILD);
    if (WIFEXITED(status) && WEXITSTATUS(status) == 0)
        return true;
    if (WIFEXITED(status))
        fprintf(stderr, "anemone-bench: %s exited with status %d\n",
            test->name, WEXITSTATUS(status));
    else if (WIFSIGNALED(status))
        fprintf(stderr, "anemone-bench: %s terminated by signal %d\n",
            test->name, WTERMSIG(status));
    else
        fprintf(stderr, "anemone-bench: %s ended with wait status %#x\n",
            test->name, status);
    return false;
}
