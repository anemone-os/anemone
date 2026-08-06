#include "harness/runtime.h"
#include "suites.h"

#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <sys/mman.h>
#include <unistd.h>

struct vm_arena {
    unsigned char *mapping;
    size_t mapping_bytes;
    volatile unsigned char *target;
    size_t target_bytes;
    size_t page_size;
};

static bool is_power_of_two(size_t value)
{
    return value && !(value & (value - 1));
}

static size_t checked_multiply(size_t left, size_t right,
    const char *operation)
{
    if (left && right > SIZE_MAX / left)
        bench_fail(operation, EOVERFLOW);
    return left * right;
}

static struct vm_arena prepare_arena(const struct vm_bench_config *config,
    bool cross_leaf)
{
    long raw_page_size = sysconf(_SC_PAGESIZE);
    struct vm_arena arena;
    size_t leaf_span_bytes;
    uintptr_t base, aligned, boundary, target;
    void *mapping;

    if (raw_page_size <= 0)
        bench_fail("sysconf(_SC_PAGESIZE)", errno ? errno : EINVAL);
    arena.page_size = (size_t)raw_page_size;
    if (!is_power_of_two(config->leaf_span_pages) ||
        config->pages > config->leaf_span_pages) {
        bench_fail("invalid VM leaf-span configuration", EINVAL);
    }
    if (cross_leaf && config->pages < 2)
        bench_fail("one page cannot cross a leaf-table boundary", EINVAL);

    leaf_span_bytes = checked_multiply(config->leaf_span_pages,
        arena.page_size, "VM leaf span overflow");
    arena.target_bytes = checked_multiply(config->pages, arena.page_size,
        "VM target range overflow");
    arena.mapping_bytes = checked_multiply(leaf_span_bytes, 4,
        "VM arena size overflow");
    mapping = mmap(NULL, arena.mapping_bytes, PROT_NONE,
        MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mapping == MAP_FAILED)
        bench_fail("mmap VM arena", errno);

    arena.mapping = mapping;
    base = (uintptr_t)mapping;
    if (base > UINTPTR_MAX - (leaf_span_bytes - 1))
        bench_fail("align VM arena", EOVERFLOW);
    aligned = (base + leaf_span_bytes - 1) & ~(leaf_span_bytes - 1);
    if (aligned > UINTPTR_MAX - leaf_span_bytes)
        bench_fail("place VM boundary", EOVERFLOW);
    boundary = aligned + leaf_span_bytes;
    if (cross_leaf) {
        target = boundary - (config->pages / 2) * arena.page_size;
    } else {
        target = boundary +
            ((config->leaf_span_pages - config->pages) / 2) * arena.page_size;
    }
    if (target < base || target > base + arena.mapping_bytes ||
        arena.target_bytes > base + arena.mapping_bytes - target) {
        bench_fail("place VM target range", EOVERFLOW);
    }
    arena.target = (volatile unsigned char *)target;
    return arena;
}

static void map_target(const struct vm_arena *arena)
{
    void *mapped = mmap((void *)arena->target, arena->target_bytes,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);

    if (mapped == MAP_FAILED)
        bench_fail("mmap VM target", errno);
    if (mapped != (void *)arena->target)
        bench_fail("mmap VM target address", EFAULT);
}

static size_t touch_target(const struct vm_arena *arena, size_t pages)
{
    size_t checksum = 0;
    size_t i;

    for (i = 0; i < pages; i++) {
        volatile unsigned char *byte = arena->target + i * arena->page_size;
        unsigned char value = (unsigned char)(i % 251 + 1);

        *byte = value;
        checksum += *byte;
    }
    return checksum;
}

static void release_arena(const struct vm_arena *arena)
{
    if (munmap(arena->mapping, arena->mapping_bytes) != 0)
        bench_fail("munmap VM arena", errno);
}

static size_t map_lifecycle(const struct vm_bench_config *config,
    bool cross_leaf)
{
    struct vm_arena arena = prepare_arena(config, cross_leaf);
    size_t checksum = 0;
    size_t iteration;

    for (iteration = 0; iteration < config->iterations; iteration++) {
        map_target(&arena);
        checksum += touch_target(&arena, config->pages);
        if (munmap((void *)arena.target, arena.target_bytes) != 0)
            bench_fail("munmap VM target", errno);
    }
    release_arena(&arena);
    return checksum;
}

static size_t protect_refault(const struct vm_bench_config *config,
    bool cross_leaf)
{
    struct vm_arena arena = prepare_arena(config, cross_leaf);
    size_t checksum = 0;
    size_t iteration, page;

    map_target(&arena);
    touch_target(&arena, config->pages);
    for (iteration = 0; iteration < config->iterations; iteration++) {
        if (mprotect((void *)arena.target, arena.target_bytes, PROT_READ) != 0)
            bench_fail("mprotect VM target read-only", errno);
        for (page = 0; page < config->pages; page++)
            checksum += arena.target[page * arena.page_size];

        if (mprotect((void *)arena.target, arena.target_bytes,
                PROT_READ | PROT_WRITE) != 0) {
            bench_fail("mprotect VM target writable", errno);
        }
        for (page = 0; page < config->pages; page++) {
            volatile unsigned char *byte =
                arena.target + page * arena.page_size;
            unsigned char value = *byte;

            *byte = value;
            checksum += *byte;
        }
    }
    release_arena(&arena);
    return checksum;
}

size_t b_vm_map_lifecycle_inside(const void *argument)
{
    return map_lifecycle(argument, false);
}

size_t b_vm_map_lifecycle_cross(const void *argument)
{
    return map_lifecycle(argument, true);
}

size_t b_vm_protect_refault_inside(const void *argument)
{
    return protect_refault(argument, false);
}

size_t b_vm_protect_refault_cross(const void *argument)
{
    return protect_refault(argument, true);
}
