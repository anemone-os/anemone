#include "harness/display.h"
#include "harness/runner.h"
#include "suites/suites.h"

#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define DEFAULT_PTHREAD_COUNT 2500
#define DEFAULT_CPU_ITERATIONS 50000000

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
    const struct bench_case cases[] = {
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
            "abcdefghijklmnopqrstuvwxyz" },
        { "string.strstr_permuted", b_string_strstr,
            "azbycxdwevfugthsirjqkplomn" },
        { "string.strstr_runs", b_string_strstr,
            "aaaaaaaaaaaaaacccccccccccc" },
        { "string.strstr_suffix4", b_string_strstr,
            "aaaaaaaaaaaaaaaaaaaaaaaaac" },
        { "string.strstr_suffix8", b_string_strstr,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaac" },
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
        { "regex.compile_default", b_regex_compile, "(a|b|c)*d*b" },
        { "regex.search_default", b_regex_search, "(a|b|c)*d*b" },
        { "regex.search_repeat", b_regex_search, "a{25}b" },
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

    bench_display_configuration(pthread_config.count, cpu_config.iterations,
        repeats);
    for (repeat = 1; repeat <= repeats; repeat++) {
        if (selected) {
            bench_display_case(selected->name, repeat, repeats);
            if (!bench_run(selected, repeat, repeats))
                return 1;
        } else {
            for (i = 0; i < case_count; i++) {
                bench_display_case(cases[i].name, repeat, repeats);
                if (!bench_run(&cases[i], repeat, repeats))
                    return 1;
            }
        }
    }
    return 0;
}
