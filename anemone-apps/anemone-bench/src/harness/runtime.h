#ifndef ANEMONE_BENCH_RUNTIME_H
#define ANEMONE_BENCH_RUNTIME_H

#include <stddef.h>

_Noreturn void bench_fail(const char *operation, int error);
void *bench_malloc(size_t size);
void bench_consume_memory(const void *data, size_t size);

#endif
