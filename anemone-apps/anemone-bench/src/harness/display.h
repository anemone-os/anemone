#ifndef ANEMONE_BENCH_DISPLAY_H
#define ANEMONE_BENCH_DISPLAY_H

#include <stddef.h>

void bench_display_configuration(size_t pthread_count, size_t cpu_iterations,
    size_t vm_pages, size_t vm_iterations, size_t vm_leaf_span_pages,
    size_t repeats);
void bench_display_case(const char *name, size_t repeat, size_t repeats);

#endif
