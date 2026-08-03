#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
    echo "usage: build.sh <c|c++> <source> <output-name> [compiler-args...]" >&2
    exit 2
fi

language=$1
source=$2
output_name=$3
shift 3

case "$ANEMONE_ARCH" in
    riscv64)
        expected_anemone_target=riscv64-unknown-anemone-elf
        c_compiler=riscv64-unknown-linux-musl-gcc
        cpp_compiler=riscv64-unknown-linux-musl-g++
        ;;
    loongarch64)
        expected_anemone_target=loongarch64-unknown-anemone-elf
        c_compiler=loongarch64-unknown-linux-musl-gcc
        cpp_compiler=loongarch64-unknown-linux-musl-g++
        ;;
    *)
        echo "unsupported ANEMONE_ARCH: $ANEMONE_ARCH" >&2
        exit 2
        ;;
esac

if [[ "$ANEMONE_TARGET_TRIPLE" != "$expected_anemone_target" ]]; then
    echo "ANEMONE_TARGET_TRIPLE '$ANEMONE_TARGET_TRIPLE' does not match '$ANEMONE_ARCH'" >&2
    exit 2
fi

# The injected triple identifies the Anemone artifact target; it is not a
# compiler flag. These fixtures deliberately use their Linux-musl toolchain
# identity because that toolchain owns the C/C++ runtime and sysroot.

case "$language" in
    c)
        compiler=${CC:-$c_compiler}
        language_flags=(-std=c11)
        ;;
    c++)
        compiler=${CXX:-$cpp_compiler}
        language_flags=(-std=c++20)
        ;;
    *)
        echo "unsupported language: $language" >&2
        exit 2
        ;;
esac

output_dir="out/$ANEMONE_ARCH"
mkdir -p "$output_dir"
"$compiler" \
    -static \
    -O2 \
    "${language_flags[@]}" \
    "$@" \
    "$source" \
    -o "$output_dir/$output_name"
