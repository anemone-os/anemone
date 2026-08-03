#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include <stdio.h>
#include <termios.h>
#include <unistd.h>

#define ANEMONE_SYS_POWER_SHUTDOWN 0x201L
#define ANEMONE_SHUTDOWN_MAGIC 0xdeadce11UL

int main(void) {
    struct termios termios;

    if (puts("command-c: guest execution ok") < 0 || fflush(stdout) != 0) {
        return 1;
    }
    if (tcgetattr(STDOUT_FILENO, &termios) != 0 ||
        tcsetattr(STDOUT_FILENO, TCSADRAIN, &termios) != 0) {
        perror("command-c: failed to drain stdout");
        return 1;
    }

    /*
     * This app is an EmbeddedApp acceptance init, so returning would violate
     * the kernel's init-task lifecycle. The syscall number and magic are the
     * Anemone native power ABI, not Linux ABI; update them with anemone-abi.
     */
    (void)syscall(ANEMONE_SYS_POWER_SHUTDOWN, ANEMONE_SHUTDOWN_MAGIC);
    perror("command-c: power shutdown returned");
    return 1;
}
