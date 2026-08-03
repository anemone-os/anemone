#include <stdio.h>

int main(void) {
    return puts("command-c: guest execution ok") < 0 ? 1 : 0;
}
