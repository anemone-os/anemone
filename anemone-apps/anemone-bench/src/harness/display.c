#include "display.h"

#include <stdio.h>
#include <unistd.h>

#define ANSI_BOLD "\033[1m"
#define ANSI_BOLD_CYAN "\033[1;36m"
#define ANSI_BOLD_BLUE "\033[1;34m"
#define ANSI_RESET "\033[0m"

static const char *ansi(const char *sequence)
{
    return isatty(STDOUT_FILENO) ? sequence : "";
}

void bench_display_configuration(size_t pthread_count, size_t cpu_iterations,
    size_t vm_pages, size_t vm_iterations, size_t vm_leaf_span_pages,
    size_t repeats)
{
    printf("%s== Anemone Bench ==%s\n",
        ansi(ANSI_BOLD_CYAN), ansi(ANSI_RESET));
    printf("  %-16s %zu\n", "pthread count", pthread_count);
    printf("  %-16s %zu\n", "CPU iterations", cpu_iterations);
    printf("  %-16s %zu\n", "VM pages", vm_pages);
    printf("  %-16s %zu\n", "VM iterations", vm_iterations);
    printf("  %-16s %zu\n", "VM leaf span", vm_leaf_span_pages);
    printf("  %-16s %zu\n", "repetitions", repeats);
    printf("  %-16s %s\n", "clock", "monotonic_raw");
    printf("ANEMONE_BENCH pthread_count=%zu cpu_iterations=%zu "
        "vm_pages=%zu vm_iterations=%zu vm_leaf_span_pages=%zu repeat=%zu "
        "clock=monotonic_raw\n",
        pthread_count, cpu_iterations, vm_pages, vm_iterations,
        vm_leaf_span_pages, repeats);
}

void bench_display_case(const char *name, size_t repeat, size_t repeats)
{
    printf("\n%s[%zu/%zu]%s %s%s%s\n",
        ansi(ANSI_BOLD_BLUE), repeat, repeats, ansi(ANSI_RESET),
        ansi(ANSI_BOLD), name, ansi(ANSI_RESET));
}
