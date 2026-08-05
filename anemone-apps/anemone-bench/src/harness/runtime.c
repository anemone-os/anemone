#include "runtime.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static volatile unsigned char consumed_byte;

_Noreturn void bench_fail(const char *operation, int error)
{
    if (error)
        fprintf(stderr, "anemone-bench: %s: %s\n", operation, strerror(error));
    else
        fprintf(stderr, "anemone-bench: %s failed\n", operation);
    exit(EXIT_FAILURE);
}

void *bench_malloc(size_t size)
{
    void *allocation = malloc(size);

    if (!allocation && size)
        bench_fail("malloc", ENOMEM);
    return allocation;
}

void bench_consume_memory(const void *data, size_t size)
{
    const unsigned char *bytes = data;

    if (size)
        consumed_byte ^= bytes[size / 2];
}
