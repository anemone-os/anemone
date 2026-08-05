#include "harness/runtime.h"
#include "suites.h"

#include <stdlib.h>
#include <string.h>

#define BUFFER_LENGTH 500000

size_t b_string_strstr(const void *argument)
{
    const char *needle = argument;
    size_t needle_length = strlen(needle);
    size_t copies = 10000;
    size_t checksum = 0;
    char *haystack;
    size_t i;

    if (!needle_length)
        bench_fail("strstr needle must not be empty", 0);
    haystack = bench_malloc(needle_length * copies + 1);
    for (i = 0; i < copies - 1; i++) {
        memcpy(haystack + needle_length * i, needle, needle_length);
        haystack[needle_length * i + needle_length - 1] ^= 1;
    }
    memcpy(haystack + needle_length * i, needle, needle_length + 1);
    for (i = 0; i < 50; i++) {
        char *match;

        haystack[0] ^= 1;
        match = strstr(haystack, needle);
        if (!match)
            bench_fail("strstr", 0);
        checksum += (size_t)(match - haystack);
    }
    free(haystack);
    return checksum;
}

size_t b_string_memset(const void *argument)
{
    unsigned char *buffer = bench_malloc(BUFFER_LENGTH);
    size_t i;
    size_t checksum;

    (void)argument;
    for (i = 0; i < 100; i++) {
        memset(buffer + i, (int)i, BUFFER_LENGTH - i);
        bench_consume_memory(buffer, BUFFER_LENGTH);
    }
    checksum = buffer[0] + buffer[BUFFER_LENGTH / 2] + buffer[BUFFER_LENGTH - 1];
    free(buffer);
    return checksum;
}

size_t b_string_strchr(const void *argument)
{
    char *buffer = bench_malloc(BUFFER_LENGTH);
    size_t checksum = 0;
    size_t i;

    (void)argument;
    memset(buffer, 'a', BUFFER_LENGTH);
    buffer[BUFFER_LENGTH - 1] = '\0';
    buffer[BUFFER_LENGTH - 2] = 'b';
    for (i = 0; i < 100; i++) {
        char *match;

        buffer[i] = (char)('0' + i % 8);
        match = strchr(buffer, 'b');
        if (!match)
            bench_fail("strchr", 0);
        checksum += (size_t)(match - buffer);
    }
    free(buffer);
    return checksum;
}

size_t b_string_strlen(const void *argument)
{
    char *buffer = bench_malloc(BUFFER_LENGTH);
    size_t checksum = 0;
    size_t i;

    (void)argument;
    memset(buffer, 'a', BUFFER_LENGTH - 1);
    buffer[BUFFER_LENGTH - 1] = '\0';
    for (i = 0; i < 100; i++) {
        buffer[i] = (char)('0' + i % 8);
        checksum += strlen(buffer);
    }
    free(buffer);
    return checksum;
}
