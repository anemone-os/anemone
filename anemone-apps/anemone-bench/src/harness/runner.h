#ifndef ANEMONE_BENCH_RUNNER_H
#define ANEMONE_BENCH_RUNNER_H

#include <stdbool.h>
#include <stddef.h>

typedef size_t (*bench_fn)(const void *);

struct bench_case {
    const char *name;
    bench_fn run;
    const void *argument;
};

bool bench_run(const struct bench_case *test, size_t repeat, size_t repeats);

#endif
