#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$script_dir"

case ${ANEMONE_ARCH:-} in
    host)
        if [[ -n ${ANEMONE_TARGET_TRIPLE+x} ]]; then
            echo "ANEMONE_TARGET_TRIPLE must be unset for host builds" >&2
            exit 2
        fi
        expected_target=
        compiler=${HOST_CC:-cc}
        ;;
    riscv64)
        expected_target=riscv64-unknown-anemone-elf
        compiler=${CC:-riscv64-unknown-linux-musl-gcc}
        ;;
    loongarch64)
        expected_target=loongarch64-unknown-anemone-elf
        compiler=${CC:-loongarch64-unknown-linux-musl-gcc}
        ;;
    *)
        echo "unsupported ANEMONE_ARCH: ${ANEMONE_ARCH:-unset}" >&2
        exit 2
        ;;
esac

if [[ -n $expected_target ]]; then
    if [[ ${ANEMONE_TARGET_TRIPLE:-} != "$expected_target" ]]; then
        echo "ANEMONE_TARGET_TRIPLE '${ANEMONE_TARGET_TRIPLE:-unset}' does not match '$expected_target'" >&2
        exit 2
    fi
fi
output_dir="out/$ANEMONE_ARCH"

mkdir -p "$output_dir"
"$compiler" \
    -static \
    -Os \
    -D_GNU_SOURCE \
    -std=c11 \
    -Wall \
    -Wextra \
    -Werror \
    -pthread \
    -Isrc \
    "$@" \
    src/*.c \
    src/harness/*.c \
    src/suites/*.c \
    -o "$output_dir/anemone-bench"
