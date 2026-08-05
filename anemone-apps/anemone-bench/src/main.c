#include "bench.h"

#include <errno.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define DEFAULT_PTHREAD_COUNT 2500
#define DEFAULT_CPU_ITERATIONS 50000000

typedef size_t (*bench_fn)(void *);

struct bench_case {
    const char *name;
    bench_fn run;
    void *argument;
};

struct heap_stats {
    bool available;
    size_t virtual_kib;
    size_t resident_kib;
    size_t private_dirty_kib;
};

static void usage(FILE *stream)
{
    fprintf(stream,
        "usage: anemone-bench [--list] [--case NAME] "
        "[--pthread-count N] [--cpu-iterations N] [--repeat N]\n");
}

static bool parse_positive_size(const char *text, size_t *value)
{
    char *end;
    unsigned long long parsed;

    errno = 0;
    parsed = strtoull(text, &end, 10);
    if (errno || !text[0] || *end || parsed == 0 || parsed > SIZE_MAX)
        return false;
    *value = (size_t)parsed;
    return true;
}

static struct heap_stats read_heap_stats(void)
{
    struct heap_stats stats = {0};
    FILE *file;
    char line[512];
    bool in_heap = false;
    unsigned long start, end, offset, inode, value;
    char permissions[5], device[16], name[256];
    int fields;

    file = fopen("/proc/self/smaps", "rb");
    if (!file)
        return stats;
    stats.available = true;
    while (fgets(line, sizeof line, file)) {
        name[0] = '\0';
        fields = sscanf(line, "%lx-%lx %4s %lx %15s %lu %255[^\n]",
            &start, &end, permissions, &offset, device, &inode, name);
        if (fields >= 6) {
            in_heap = !strcmp(device, "00:00") && strcmp(permissions, "---p") &&
                (fields == 6 || strstr(name, "[heap]"));
            continue;
        }
        if (!in_heap)
            continue;
        if (sscanf(line, "Size: %lu", &value) == 1)
            stats.virtual_kib += value;
        else if (sscanf(line, "Rss: %lu", &value) == 1)
            stats.resident_kib += value;
        else if (sscanf(line, "Private_Dirty: %lu", &value) == 1)
            stats.private_dirty_kib += value;
    }
    if (ferror(file))
        bench_fail("read /proc/self/smaps", errno ? errno : EIO);
    if (fclose(file) == EOF)
        bench_fail("close /proc/self/smaps", errno);
    return stats;
}

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

static void run_child(const struct bench_case *test, size_t repeat, size_t repeats)
{
    struct timespec begin, end;
    struct heap_stats stats;
    size_t checksum;
    uint64_t elapsed;

    if (clock_gettime(CLOCK_MONOTONIC_RAW, &begin) != 0)
        bench_fail("clock_gettime(begin)", errno);
    checksum = test->run(test->argument);
    if (clock_gettime(CLOCK_MONOTONIC_RAW, &end) != 0)
        bench_fail("clock_gettime(end)", errno);
    elapsed = elapsed_nanoseconds(begin, end);
    stats = read_heap_stats();

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

static bool run_bench(const struct bench_case *test, size_t repeat, size_t repeats)
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

int main(int argc, char **argv)
{
    const char *selected_name = NULL;
    struct pthread_bench_config pthread_config = {
        .count = DEFAULT_PTHREAD_COUNT,
    };
    struct cpu_bench_config cpu_config = {
        .iterations = DEFAULT_CPU_ITERATIONS,
    };
    size_t repeats = 1;
    bool list = false;
    bool help = false;
    size_t i, repeat;
    const struct bench_case *selected = NULL;
    struct bench_case cases[] = {
        { "cpu.integer", b_cpu_integer, &cpu_config },
        { "cpu.floating_point", b_cpu_floating_point, &cpu_config },
        { "malloc.sparse", b_malloc_sparse, NULL },
        { "malloc.bubble", b_malloc_bubble, NULL },
        { "malloc.tiny1", b_malloc_tiny1, NULL },
        { "malloc.tiny2", b_malloc_tiny2, NULL },
        { "malloc.big1", b_malloc_big1, NULL },
        { "malloc.big2", b_malloc_big2, NULL },
        { "malloc.thread_stress", b_malloc_thread_stress, NULL },
        { "malloc.thread_local", b_malloc_thread_local, NULL },
        { "string.strstr_forward", b_string_strstr,
            (void *)"abcdefghijklmnopqrstuvwxyz" },
        { "string.strstr_permuted", b_string_strstr,
            (void *)"azbycxdwevfugthsirjqkplomn" },
        { "string.strstr_runs", b_string_strstr,
            (void *)"aaaaaaaaaaaaaacccccccccccc" },
        { "string.strstr_suffix4", b_string_strstr,
            (void *)"aaaaaaaaaaaaaaaaaaaaaaaaac" },
        { "string.strstr_suffix8", b_string_strstr,
            (void *)"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaac" },
        { "string.memset", b_string_memset, NULL },
        { "string.strchr", b_string_strchr, NULL },
        { "string.strlen", b_string_strlen, NULL },
        { "pthread.createjoin_serial1", b_pthread_createjoin_serial1,
            &pthread_config },
        { "pthread.createjoin_serial2", b_pthread_createjoin_serial2,
            &pthread_config },
        { "pthread.create_serial1", b_pthread_create_serial1,
            &pthread_config },
        { "pthread.uselesslock", b_pthread_uselesslock, NULL },
        { "pthread.createjoin_minimal1", b_pthread_createjoin_minimal1,
            &pthread_config },
        { "pthread.createjoin_minimal2", b_pthread_createjoin_minimal2,
            &pthread_config },
        { "utf8.bigbuf", b_utf8_bigbuf, NULL },
        { "utf8.onebyone", b_utf8_onebyone, NULL },
        { "stdio.putcgetc", b_stdio_putcgetc, NULL },
        { "stdio.putcgetc_unlocked", b_stdio_putcgetc_unlocked, NULL },
        { "regex.compile_default", b_regex_compile, (void *)"(a|b|c)*d*b" },
        { "regex.search_default", b_regex_search, (void *)"(a|b|c)*d*b" },
        { "regex.search_repeat", b_regex_search, (void *)"a{25}b" },
    };
    const size_t case_count = sizeof cases / sizeof cases[0];

    for (i = 1; i < (size_t)argc; i++) {
        if (!strcmp(argv[i], "--help") || !strcmp(argv[i], "-h")) {
            help = true;
        } else if (!strcmp(argv[i], "--list")) {
            list = true;
        } else if (!strcmp(argv[i], "--case")) {
            if (++i >= (size_t)argc) {
                usage(stderr);
                return 2;
            }
            selected_name = argv[i];
        } else if (!strcmp(argv[i], "--pthread-count")) {
            if (++i >= (size_t)argc ||
                !parse_positive_size(argv[i], &pthread_config.count)) {
                fprintf(stderr, "anemone-bench: invalid --pthread-count\n");
                return 2;
            }
        } else if (!strcmp(argv[i], "--cpu-iterations")) {
            if (++i >= (size_t)argc ||
                !parse_positive_size(argv[i], &cpu_config.iterations)) {
                fprintf(stderr, "anemone-bench: invalid --cpu-iterations\n");
                return 2;
            }
        } else if (!strcmp(argv[i], "--repeat")) {
            if (++i >= (size_t)argc ||
                !parse_positive_size(argv[i], &repeats)) {
                fprintf(stderr, "anemone-bench: invalid --repeat\n");
                return 2;
            }
        } else {
            fprintf(stderr, "anemone-bench: unknown argument: %s\n", argv[i]);
            usage(stderr);
            return 2;
        }
    }

    if (help) {
        usage(stdout);
        return 0;
    }
    if (list) {
        for (i = 0; i < case_count; i++)
            puts(cases[i].name);
        return 0;
    }
    if (selected_name) {
        for (i = 0; i < case_count; i++) {
            if (!strcmp(cases[i].name, selected_name)) {
                selected = &cases[i];
                break;
            }
        }
        if (!selected) {
            fprintf(stderr, "anemone-bench: unknown case: %s\n", selected_name);
            return 2;
        }
    }

    printf("ANEMONE_BENCH pthread_count=%zu cpu_iterations=%zu repeat=%zu "
        "clock=monotonic_raw\n",
        pthread_config.count, cpu_config.iterations, repeats);
    for (repeat = 1; repeat <= repeats; repeat++) {
        if (selected) {
            if (!run_bench(selected, repeat, repeats))
                return 1;
        } else {
            for (i = 0; i < case_count; i++) {
                if (!run_bench(&cases[i], repeat, repeats))
                    return 1;
            }
        }
    }
    return 0;
}
