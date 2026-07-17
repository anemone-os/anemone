#include "test-lib.h"
#include <stdio.h>

int main()
{
    const char *test_name = "C Test";

    test_start(test_name);

    printf("Hello, Anemone!\n");

    test_end(test_name);

    return 0;
}