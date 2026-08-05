#include "bench.h"

#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <stddef.h>
#include <unistd.h>

#define THREAD_BATCH_SIZE 50

static void pthread_check(const char *operation, int error)
{
    if (error)
        bench_fail(operation, error);
}

static void *emptyfunc(void *argument)
{
    (void)argument;
    return NULL;
}

static size_t createjoin_serial(const struct pthread_bench_config *config,
    const pthread_attr_t *attributes)
{
    size_t i;
    pthread_t thread;
    void *result;

    for (i = 0; i < config->count; i++) {
        pthread_check("pthread_create", pthread_create(&thread, attributes,
            emptyfunc, NULL));
        pthread_check("pthread_join", pthread_join(thread, &result));
    }
    return config->count;
}

static size_t createjoin_batched(const struct pthread_bench_config *config,
    const pthread_attr_t *attributes)
{
    pthread_t threads[THREAD_BATCH_SIZE];
    size_t completed = 0;
    void *result;

    while (completed < config->count) {
        size_t remaining = config->count - completed;
        size_t batch = remaining < THREAD_BATCH_SIZE ? remaining : THREAD_BATCH_SIZE;
        size_t i;

        for (i = 0; i < batch; i++)
            pthread_check("pthread_create", pthread_create(&threads[i], attributes,
                emptyfunc, NULL));
        for (i = 0; i < batch; i++)
            pthread_check("pthread_join", pthread_join(threads[i], &result));
        completed += batch;
    }
    return completed;
}

size_t b_pthread_createjoin_serial1(void *argument)
{
    return createjoin_serial(argument, NULL);
}

size_t b_pthread_createjoin_serial2(void *argument)
{
    return createjoin_batched(argument, NULL);
}

size_t b_pthread_create_serial1(void *argument)
{
    const struct pthread_bench_config *config = argument;
    pthread_attr_t attributes;
    pthread_t thread;
    size_t i;

    pthread_check("pthread_attr_init", pthread_attr_init(&attributes));
    pthread_check("pthread_attr_setstacksize",
        pthread_attr_setstacksize(&attributes, 16384));
    /* This upstream case measures create-only pressure; joining changes it. */
    for (i = 0; i < config->count; i++)
        pthread_check("pthread_create", pthread_create(&thread, &attributes,
            emptyfunc, NULL));
    pthread_check("pthread_attr_destroy", pthread_attr_destroy(&attributes));
    return config->count;
}

static void *lockunlock(void *argument)
{
    pthread_mutex_t *mutex = argument;
    size_t i;

    for (i = 0; i < 1000000; i++) {
        /* The valid private mutex cannot fail; preserve the upstream hot loop. */
        pthread_mutex_lock(mutex);
        pthread_mutex_unlock(mutex);
    }
    return NULL;
}

size_t b_pthread_uselesslock(void *argument)
{
    pthread_t thread;
    pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
    void *result;

    (void)argument;
    pthread_check("pthread_create",
        pthread_create(&thread, NULL, lockunlock, &mutex));
    pthread_check("pthread_join", pthread_join(thread, &result));
    pthread_check("pthread_mutex_destroy", pthread_mutex_destroy(&mutex));
    return 1000000;
}

static void minimal_attributes(pthread_attr_t *attributes)
{
    long page_size = sysconf(_SC_PAGESIZE);
    size_t stack_size;

    if (page_size <= 0)
        bench_fail("sysconf(_SC_PAGESIZE)", errno ? errno : EINVAL);
    stack_size = (size_t)page_size;
    if (stack_size < (size_t)PTHREAD_STACK_MIN)
        stack_size = (size_t)PTHREAD_STACK_MIN;
    pthread_check("pthread_attr_init", pthread_attr_init(attributes));
    pthread_check("pthread_attr_setstacksize",
        pthread_attr_setstacksize(attributes, stack_size));
    pthread_check("pthread_attr_setguardsize",
        pthread_attr_setguardsize(attributes, 0));
}

size_t b_pthread_createjoin_minimal1(void *argument)
{
    pthread_attr_t attributes;
    size_t completed;

    minimal_attributes(&attributes);
    completed = createjoin_serial(argument, &attributes);
    pthread_check("pthread_attr_destroy", pthread_attr_destroy(&attributes));
    return completed;
}

size_t b_pthread_createjoin_minimal2(void *argument)
{
    pthread_attr_t attributes;
    size_t completed;

    minimal_attributes(&attributes);
    completed = createjoin_batched(argument, &attributes);
    pthread_check("pthread_attr_destroy", pthread_attr_destroy(&attributes));
    return completed;
}
