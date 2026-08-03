#!/usr/bin/env bash
set -euo pipefail

case "$ANEMONE_ARCH" in
    riscv64)
        expected_anemone_target=riscv64-unknown-anemone-elf
        default_compiler=riscv64-unknown-linux-musl-gcc
        ;;
    loongarch64)
        expected_anemone_target=loongarch64-unknown-anemone-elf
        default_compiler=loongarch64-unknown-linux-musl-gcc
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

# The injected triple identifies the Anemone artifact target; the Linux-musl
# compiler identity remains responsible for the C runtime and sysroot.
compiler=${CC:-$default_compiler}
output_dir="out/$ANEMONE_ARCH"
mkdir -p "$output_dir"
"$compiler" \
    -static \
    -O2 \
    -std=c11 \
    "$@" \
    main.c \
    -o "$output_dir/command-c"
