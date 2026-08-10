#include "harness/runtime.h"
#include "suites.h"

#include <pthread.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

#define ARRAY_SIZE(array) (sizeof(array) / sizeof((array)[0]))
#define LOOPS 100000
#define SHARED_COUNT 300
#define MAX_SIZE 500

static void pthread_check(const char *operation, int error)
{
    if (error)
        bench_fail(operation, error);
}

size_t b_malloc_sparse(const void *argument)
{
    void *allocations[10000];
    size_t i;

    (void)argument;
    for (i = 0; i < ARRAY_SIZE(allocations); i++) {
        allocations[i] = bench_malloc(4000);
        memset(allocations[i], 0, 4000);
    }
    /* Retained blocks are the upstream fragmentation payload, not a leak bug. */
    for (i = 0; i < ARRAY_SIZE(allocations); i++) {
        if (i % 150)
            free(allocations[i]);
    }
    return (ARRAY_SIZE(allocations) + 149) / 150;
}

size_t b_malloc_bubble(const void *argument)
{
    void *allocations[10000];
    size_t i;

    (void)argument;
    for (i = 0; i < ARRAY_SIZE(allocations); i++) {
        allocations[i] = bench_malloc(4000);
        memset(allocations[i], 0, 4000);
    }
    /* The final live block keeps the upstream heap-stat workload intact. */
    for (i = 0; i < ARRAY_SIZE(allocations) - 1; i++)
        free(allocations[i]);
    return 1;
}

static size_t malloc_tiny(bool permuted)
{
    void *allocations[10000];
    size_t i;

    for (i = 0; i < ARRAY_SIZE(allocations); i++)
        allocations[i] = bench_malloc((i % 4 + 1) * 16);
    if (permuted) {
        /* The permutation intentionally visits every index except zero. */
        for (i = 1; i; i = (i + 57) % ARRAY_SIZE(allocations))
            free(allocations[i]);
        return ARRAY_SIZE(allocations) - 1;
    }
    for (i = 0; i < ARRAY_SIZE(allocations); i++)
        free(allocations[i]);
    return ARRAY_SIZE(allocations);
}

size_t b_malloc_tiny1(const void *argument)
{
    (void)argument;
    return malloc_tiny(false);
}

size_t b_malloc_tiny2(const void *argument)
{
    (void)argument;
    return malloc_tiny(true);
}

static size_t malloc_big(bool permuted)
{
    void *allocations[2000];
    size_t i;

    for (i = 0; i < ARRAY_SIZE(allocations); i++)
        allocations[i] = bench_malloc((i % 4 + 1) * 16384);
    if (permuted) {
        /* The permutation intentionally visits every index except zero. */
        for (i = 1; i; i = (i + 57) % ARRAY_SIZE(allocations))
            free(allocations[i]);
        return ARRAY_SIZE(allocations) - 1;
    }
    for (i = 0; i < ARRAY_SIZE(allocations); i++)
        free(allocations[i]);
    return ARRAY_SIZE(allocations);
}

size_t b_malloc_big1(const void *argument)
{
    (void)argument;
    return malloc_big(false);
}

size_t b_malloc_big2(const void *argument)
{
    (void)argument;
    return malloc_big(true);
}

struct allocation_slot {
    pthread_mutex_t lock;
    void *memory;
};

struct stress_argument {
    struct allocation_slot *slots;
    unsigned random;
};

static unsigned next_random(unsigned *random)
{
    return *random = *random * 1103515245 + 12345;
}

static void *stress(void *argument)
{
    struct stress_argument *stress_argument = argument;
    struct allocation_slot *slots = stress_argument->slots;
    unsigned random = stress_argument->random;
    size_t i;

    for (i = 0; i < LOOPS; i++) {
        size_t index = next_random(&random) % SHARED_COUNT;
        size_t size = next_random(&random) % MAX_SIZE;
        void *memory;

        /* Keep the upstream hot path free of measurement-only branches. */
        pthread_mutex_lock(&slots[index].lock);
        memory = slots[index].memory;
        slots[index].memory = NULL;
        pthread_mutex_unlock(&slots[index].lock);
        free(memory);
        if (!memory) {
            memory = bench_malloc(size);
            pthread_mutex_lock(&slots[index].lock);
            if (!slots[index].memory) {
                slots[index].memory = memory;
                memory = NULL;
            }
            pthread_mutex_unlock(&slots[index].lock);
            free(memory);
        }
    }
    return NULL;
}

static void init_slots(struct allocation_slot *slots)
{
    size_t i;

    for (i = 0; i < SHARED_COUNT; i++) {
        slots[i].memory = NULL;
        pthread_check("pthread_mutex_init", pthread_mutex_init(&slots[i].lock, NULL));
    }
}

static void destroy_slots(struct allocation_slot *slots)
{
    size_t i;

    for (i = 0; i < SHARED_COUNT; i++) {
        free(slots[i].memory);
        pthread_check("pthread_mutex_destroy", pthread_mutex_destroy(&slots[i].lock));
    }
}

static size_t run_thread_stress(struct allocation_slot *first_slots,
    struct allocation_slot *second_slots)
{
    struct stress_argument first = {
        .slots = first_slots,
        .random = 1,
    };
    struct stress_argument second = {
        .slots = second_slots,
        .random = 2,
    };
    pthread_t first_thread, second_thread;
    void *result;

    pthread_check("pthread_create",
        pthread_create(&first_thread, NULL, stress, &first));
    pthread_check("pthread_create",
        pthread_create(&second_thread, NULL, stress, &second));
    pthread_check("pthread_join", pthread_join(first_thread, &result));
    pthread_check("pthread_join", pthread_join(second_thread, &result));
    return LOOPS * 2;
}

size_t b_malloc_thread_stress(const void *argument)
{
    struct allocation_slot slots[SHARED_COUNT];
    size_t checksum;

    (void)argument;
    init_slots(slots);
    checksum = run_thread_stress(slots, slots);
    destroy_slots(slots);
    return checksum;
}

size_t b_malloc_thread_local(const void *argument)
{
    struct allocation_slot first_slots[SHARED_COUNT];
    struct allocation_slot second_slots[SHARED_COUNT];
    size_t checksum;

    (void)argument;
    init_slots(first_slots);
    init_slots(second_slots);
    checksum = run_thread_stress(first_slots, second_slots);
    destroy_slots(second_slots);
    destroy_slots(first_slots);
    return checksum;
}
