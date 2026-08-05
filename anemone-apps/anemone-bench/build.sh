#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$script_dir"

if [[ ${1:-} == --host ]]; then
    compiler=${HOST_CC:-cc}
    output_dir=out/host
    shift
else
    case ${ANEMONE_ARCH:-} in
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

    if [[ ${ANEMONE_TARGET_TRIPLE:-} != "$expected_target" ]]; then
        echo "ANEMONE_TARGET_TRIPLE '${ANEMONE_TARGET_TRIPLE:-unset}' does not match '$expected_target'" >&2
        exit 2
    fi
    output_dir="out/$ANEMONE_ARCH"
fi

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
    -Iinclude \
    "$@" \
    src/*.c \
    -o "$output_dir/anemone-bench"
