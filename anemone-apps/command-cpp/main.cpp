#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include <cstdio>
#include <iostream>
#include <string_view>
#include <termios.h>
#include <unistd.h>

constexpr long ANEMONE_SYS_POWER_SHUTDOWN = 0x201;
constexpr unsigned long ANEMONE_SHUTDOWN_MAGIC = 0xdeadce11;

template <std::size_t Size>
constexpr std::string_view payload(const char (&text)[Size]) {
    return {text, Size - 1};
}

int main() {
    static constexpr char marker[] = "command-cpp: guest execution ok";
    struct termios termios;

    std::cout << payload(marker) << std::endl;
    if (!std::cout.good()) {
        return 1;
    }
    if (::tcgetattr(STDOUT_FILENO, &termios) != 0 ||
        ::tcsetattr(STDOUT_FILENO, TCSADRAIN, &termios) != 0) {
        std::perror("command-cpp: failed to drain stdout");
        return 1;
    }

    /*
     * This app is an EmbeddedApp acceptance init, so returning would violate
     * the kernel's init-task lifecycle. The syscall number and magic are the
     * Anemone native power ABI, not Linux ABI; update them with anemone-abi.
     */
    (void)::syscall(ANEMONE_SYS_POWER_SHUTDOWN, ANEMONE_SHUTDOWN_MAGIC);
    std::perror("command-cpp: power shutdown returned");
    return 1;
}
