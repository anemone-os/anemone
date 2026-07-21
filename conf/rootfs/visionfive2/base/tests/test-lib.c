#include <stdio.h>
#include <time.h>
#include <string.h>

// 全局变量记录起始时间
static struct timespec start_time;

// 获取当前时间的格式化字符串
static void get_now_str(char *buf, size_t len) {
    time_t now;
    struct tm *tm_info;
    time(&now);
    tm_info = localtime(&now);
    strftime(buf, len, "%Y-%m-%d %H:%M:%S", tm_info);
}

void test_start(const char* name) {
    char time_buf[64];
    get_now_str(time_buf, sizeof(time_buf));

    // 记录高精度起始时间
    clock_gettime(CLOCK_MONOTONIC, &start_time);

    printf("\n###### Anemone Test Start ######\n");
    printf("Test Name: %s\n", name);
    printf("Start: %s\n", time_buf);
    printf("--------------------------------\n");
}

void test_end(const char *name) {
    struct timespec end_time;
    char time_buf[64];

    // 获取结束时间
    clock_gettime(CLOCK_MONOTONIC, &end_time);
    get_now_str(time_buf, sizeof(time_buf));

    // 计算毫秒差值
    long ms = (end_time.tv_sec - start_time.tv_sec) * 1000 +
              (end_time.tv_nsec - start_time.tv_nsec) / 1000000;

    printf("--------------------------------\n");
    printf("###### Anemone Test End ######\n");
    printf("Test Name: %s\n", name);
    printf("End: %s\n", time_buf);
    printf("Take %ld ms\n\n", ms);
}
