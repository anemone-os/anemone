#include "harness/runtime.h"
#include "suites.h"

#include <stdint.h>
#include <string.h>

_Static_assert(sizeof(double) == sizeof(uint64_t),
    "CPU floating-point checksum requires a 64-bit double");

size_t b_cpu_integer(const void *argument)
{
    const struct cpu_bench_config *config = argument;
    uint64_t first = UINT64_C(0x243f6a8885a308d3);
    uint64_t second = UINT64_C(0x13198a2e03707344);
    size_t i;

    for (i = 0; i < config->iterations; i++) {
        first ^= first << 13;
        first ^= first >> 7;
        first ^= first << 17;
        first += second;
        second = (second << 9 | second >> 55) ^ first;
    }
    return (size_t)(first ^ second);
}

size_t b_cpu_floating_point(const void *argument)
{
    const struct cpu_bench_config *config = argument;
    double first = 0.75;
    double second = 0.25;
    uint64_t bits;
    size_t i;

    for (i = 0; i < config->iterations; i++) {
        first = first * 1.00000011920928955078125 + second * 0.0000003;
        second = second * 0.9999997 + first * 0.0000002;
        if (first > 2.0)
            first -= 1.0;
    }
    memcpy(&bits, &first, sizeof bits);
    return (size_t)(bits ^ (bits >> 32));
}
