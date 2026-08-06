#include "memory.h"

#include "runtime.h"

#include <errno.h>
#include <stdio.h>
#include <string.h>

struct bench_heap_stats bench_read_heap_stats(void)
{
    struct bench_heap_stats stats = {0};
    FILE *file;
    char line[512];
    bool in_heap = false;
    unsigned long start, end, offset, inode, value;
    char permissions[5], device[16], name[256];
    int fields;

    file = fopen("/proc/self/smaps", "rb");
    if (!file)
        return stats;
    stats.available = true;
    while (fgets(line, sizeof line, file)) {
        name[0] = '\0';
        fields = sscanf(line, "%lx-%lx %4s %lx %15s %lu %255[^\n]",
            &start, &end, permissions, &offset, device, &inode, name);
        if (fields >= 6) {
            /* Allocators use both the brk heap and unnamed anonymous maps. */
            in_heap = !strcmp(device, "00:00") && strcmp(permissions, "---p") &&
                (fields == 6 || strstr(name, "[heap]"));
            continue;
        }
        if (!in_heap)
            continue;
        if (sscanf(line, "Size: %lu", &value) == 1)
            stats.virtual_kib += value;
        else if (sscanf(line, "Rss: %lu", &value) == 1)
            stats.resident_kib += value;
        else if (sscanf(line, "Private_Dirty: %lu", &value) == 1)
            stats.private_dirty_kib += value;
    }
    if (ferror(file))
        bench_fail("read /proc/self/smaps", errno ? errno : EIO);
    if (fclose(file) == EOF)
        bench_fail("close /proc/self/smaps", errno);
    return stats;
}
