#include "bench.h"

#include <errno.h>
#include <stdio.h>

#define IO_COUNT 5000000

static FILE *open_temporary_file(void)
{
    FILE *file = tmpfile();

    if (!file)
        bench_fail("tmpfile", errno);
    return file;
}

size_t b_stdio_putcgetc(void *argument)
{
    FILE *file = open_temporary_file();
    size_t checksum = 0;
    size_t i;

    (void)argument;
    for (i = 0; i < IO_COUNT; i++) {
        if (putc('x', file) == EOF)
            bench_fail("putc", errno ? errno : EIO);
    }
    if (fseeko(file, 0, SEEK_SET) != 0)
        bench_fail("fseeko", errno);
    for (i = 0; i < IO_COUNT; i++) {
        int byte = getc(file);

        if (byte == EOF)
            bench_fail("getc", ferror(file) ? (errno ? errno : EIO) : ENODATA);
        checksum += (unsigned char)byte;
    }
    if (fclose(file) == EOF)
        bench_fail("fclose", errno);
    return checksum;
}

size_t b_stdio_putcgetc_unlocked(void *argument)
{
    FILE *file = open_temporary_file();
    size_t checksum = 0;
    size_t i;

    (void)argument;
    for (i = 0; i < IO_COUNT; i++) {
        if (putc_unlocked('x', file) == EOF)
            bench_fail("putc_unlocked", errno ? errno : EIO);
    }
    if (fseeko(file, 0, SEEK_SET) != 0)
        bench_fail("fseeko", errno);
    for (i = 0; i < IO_COUNT; i++) {
        int byte = getc_unlocked(file);

        if (byte == EOF)
            bench_fail("getc_unlocked",
                ferror(file) ? (errno ? errno : EIO) : ENODATA);
        checksum += (unsigned char)byte;
    }
    if (fclose(file) == EOF)
        bench_fail("fclose", errno);
    return checksum;
}
