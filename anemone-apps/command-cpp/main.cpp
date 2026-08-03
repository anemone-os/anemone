#include <iostream>
#include <string_view>

template <std::size_t Size>
constexpr std::string_view payload(const char (&text)[Size]) {
    return {text, Size - 1};
}

int main() {
    static constexpr char marker[] = "command-cpp: guest execution ok";
    std::cout << payload(marker) << '\n';
    return std::cout.good() ? 0 : 1;
}
