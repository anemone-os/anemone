#ifndef ANEMONE_BENCH_H
#define ANEMONE_BENCH_H

#include <stddef.h>

struct pthread_bench_config {
    size_t count;
};

struct cpu_bench_config {
    size_t iterations;
};

_Noreturn void bench_fail(const char *operation, int error);
void *bench_malloc(size_t size);
void bench_consume_memory(const void *data, size_t size);

size_t b_cpu_integer(void *argument);
size_t b_cpu_floating_point(void *argument);
size_t b_malloc_sparse(void *argument);
size_t b_malloc_bubble(void *argument);
size_t b_malloc_tiny1(void *argument);
size_t b_malloc_tiny2(void *argument);
size_t b_malloc_big1(void *argument);
size_t b_malloc_big2(void *argument);
size_t b_malloc_thread_stress(void *argument);
size_t b_malloc_thread_local(void *argument);
size_t b_pthread_createjoin_serial1(void *argument);
size_t b_pthread_createjoin_serial2(void *argument);
size_t b_pthread_create_serial1(void *argument);
size_t b_pthread_uselesslock(void *argument);
size_t b_pthread_createjoin_minimal1(void *argument);
size_t b_pthread_createjoin_minimal2(void *argument);
size_t b_regex_compile(void *argument);
size_t b_regex_search(void *argument);
size_t b_stdio_putcgetc(void *argument);
size_t b_stdio_putcgetc_unlocked(void *argument);
size_t b_string_strstr(void *argument);
size_t b_string_memset(void *argument);
size_t b_string_strchr(void *argument);
size_t b_string_strlen(void *argument);
size_t b_utf8_bigbuf(void *argument);
size_t b_utf8_onebyone(void *argument);

#endif
