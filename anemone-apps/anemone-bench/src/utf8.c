#include "bench.h"

#include <errno.h>
#include <langinfo.h>
#include <locale.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define UTF8_BUFFER_SIZE 500000

static void select_utf8_locale(void)
{
    static const char *const candidates[] = {
        "C.UTF-8",
        "en_US.UTF-8",
        "en_GB.UTF-8",
        "en.UTF-8",
        "de_DE-8",
        "fr_FR-8",
    };
    size_t i;

    for (i = 0; i < sizeof candidates / sizeof candidates[0]; i++) {
        if (setlocale(LC_CTYPE, candidates[i]) &&
            !strcmp(nl_langinfo(CODESET), "UTF-8"))
            return;
    }
    bench_fail("UTF-8 locale unavailable", ENOENT);
}

static size_t fill_utf8_buffer(char *buffer)
{
    size_t i, j, k, length = 0;

    for (i = 0xc3; i < 0xe0; i++) {
        for (j = 0x80; j < 0xc0; j++) {
            buffer[length++] = (char)i;
            buffer[length++] = (char)j;
        }
    }
    for (i = 0xe1; i < 0xed; i++) {
        for (j = 0x80; j < 0xc0; j++) {
            for (k = 0x80; k < 0xc0; k++) {
                buffer[length++] = (char)i;
                buffer[length++] = (char)j;
                buffer[length++] = (char)k;
            }
        }
    }
    for (i = 0xf1; i < 0xf4; i++) {
        for (j = 0x80; j < 0xc0; j++) {
            for (k = 0x80; k < 0xc0; k++) {
                buffer[length++] = (char)i;
                buffer[length++] = (char)j;
                buffer[length++] = (char)0x80;
                buffer[length++] = (char)k;
            }
        }
    }
    if (length >= UTF8_BUFFER_SIZE)
        bench_fail("UTF-8 buffer capacity", EOVERFLOW);
    buffer[length] = '\0';
    return length;
}

size_t b_utf8_bigbuf(void *argument)
{
    char *buffer = bench_malloc(UTF8_BUFFER_SIZE);
    wchar_t *wide_buffer = bench_malloc(UTF8_BUFFER_SIZE * sizeof(*wide_buffer));
    size_t checksum = 0;
    size_t i;

    (void)argument;
    select_utf8_locale();
    fill_utf8_buffer(buffer);
    for (i = 0; i < 50; i++) {
        size_t converted = mbstowcs(wide_buffer, buffer, UTF8_BUFFER_SIZE);

        if (converted == (size_t)-1)
            bench_fail("mbstowcs", errno ? errno : EILSEQ);
        checksum += converted;
    }
    free(wide_buffer);
    free(buffer);
    return checksum;
}

size_t b_utf8_onebyone(void *argument)
{
    char *buffer = bench_malloc(UTF8_BUFFER_SIZE);
    size_t checksum = 0;
    size_t length;
    size_t i;

    (void)argument;
    select_utf8_locale();
    length = fill_utf8_buffer(buffer);
    for (i = 0; i < 50; i++) {
        mbstate_t conversion = {0};
        size_t offset = 0;

        while (offset < length) {
            wchar_t wide_character;
            size_t converted = mbrtowc(&wide_character, buffer + offset,
                length - offset, &conversion);

            if (converted == (size_t)-1 || converted == (size_t)-2 || !converted)
                bench_fail("mbrtowc", errno ? errno : EILSEQ);
            offset += converted;
            checksum += (uint32_t)wide_character;
        }
    }
    free(buffer);
    return checksum;
}
