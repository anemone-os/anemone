#include "bench.h"

#include <errno.h>
#include <locale.h>
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static _Noreturn void regex_fail(const char *operation, int error,
    const regex_t *expression)
{
    char message[256];

    regerror(error, expression, message, sizeof message);
    fprintf(stderr, "anemone-bench: %s: %s\n", operation, message);
    exit(EXIT_FAILURE);
}

static void select_environment_locale(void)
{
    if (!setlocale(LC_CTYPE, ""))
        bench_fail("setlocale", errno ? errno : EINVAL);
}

size_t b_regex_compile(void *argument)
{
    const char *pattern = argument;
    size_t checksum = 0;
    size_t i;

    select_environment_locale();
    for (i = 0; i < 1000; i++) {
        regex_t expression;
        int error = regcomp(&expression, pattern, REG_EXTENDED);

        if (error)
            regex_fail("regcomp", error, NULL);
        checksum += expression.re_nsub;
        regfree(&expression);
    }
    return checksum;
}

size_t b_regex_search(void *argument)
{
    const char *pattern = argument;
    char buffer[260000];
    regex_t expression;
    int error;

    select_environment_locale();
    memset(buffer, 'a', sizeof buffer - 2);
    buffer[sizeof buffer - 2] = 'b';
    buffer[sizeof buffer - 1] = '\0';
    error = regcomp(&expression, pattern, REG_EXTENDED);
    if (error)
        regex_fail("regcomp", error, NULL);
    error = regexec(&expression, buffer, 0, NULL, 0);
    if (error)
        regex_fail("regexec", error, &expression);
    regfree(&expression);
    return sizeof buffer - 1;
}
