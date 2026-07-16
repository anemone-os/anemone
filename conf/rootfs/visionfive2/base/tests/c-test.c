#include "test-lib.h"
#include <stdio.h>

int main()
{
    const char *test_name = "C Test";

    // 使用你要求的库函数
    test_start(test_name);

    // 核心输出内容
    printf("Hello, Anemone!\n");

    test_end(test_name);

    return 0;
}