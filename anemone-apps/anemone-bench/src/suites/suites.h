#ifndef ANEMONE_BENCH_SUITES_H
#define ANEMONE_BENCH_SUITES_H

#include <stddef.h>

struct pthread_bench_config {
    size_t count;
};

struct cpu_bench_config {
    size_t iterations;
};

struct vm_bench_config {
    size_t pages;
    size_t iterations;
    size_t leaf_span_pages;
};

size_t b_cpu_integer(const void *argument);
size_t b_cpu_floating_point(const void *argument);
size_t b_malloc_sparse(const void *argument);
size_t b_malloc_bubble(const void *argument);
size_t b_malloc_tiny1(const void *argument);
size_t b_malloc_tiny2(const void *argument);
size_t b_malloc_big1(const void *argument);
size_t b_malloc_big2(const void *argument);
size_t b_malloc_thread_stress(const void *argument);
size_t b_malloc_thread_local(const void *argument);
size_t b_pthread_createjoin_serial1(const void *argument);
size_t b_pthread_createjoin_serial2(const void *argument);
size_t b_pthread_create_serial1(const void *argument);
size_t b_pthread_uselesslock(const void *argument);
size_t b_pthread_createjoin_minimal1(const void *argument);
size_t b_pthread_createjoin_minimal2(const void *argument);
size_t b_regex_compile(const void *argument);
size_t b_regex_search(const void *argument);
size_t b_stdio_putcgetc(const void *argument);
size_t b_stdio_putcgetc_unlocked(const void *argument);
size_t b_string_strstr(const void *argument);
size_t b_string_memset(const void *argument);
size_t b_string_strchr(const void *argument);
size_t b_string_strlen(const void *argument);
size_t b_utf8_bigbuf(const void *argument);
size_t b_utf8_onebyone(const void *argument);
size_t b_vm_map_lifecycle_inside(const void *argument);
size_t b_vm_map_lifecycle_cross(const void *argument);
size_t b_vm_protect_refault_inside(const void *argument);
size_t b_vm_protect_refault_cross(const void *argument);

#endif
