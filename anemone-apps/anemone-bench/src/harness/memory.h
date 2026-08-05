#ifndef ANEMONE_BENCH_MEMORY_H
#define ANEMONE_BENCH_MEMORY_H

#include <stdbool.h>
#include <stddef.h>

struct bench_heap_stats {
    bool available;
    size_t virtual_kib;
    size_t resident_kib;
    size_t private_dirty_kib;
};

struct bench_heap_stats bench_read_heap_stats(void);

#endif
