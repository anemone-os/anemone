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
    echo "ANEMONE_TARGET_TRIPLE '$ANEMONE_TARGET_TRIPLE' does not match '$expected_anemone_target'" >&2
    exit 2
fi

# The Anemone target triple identifies the guest artifact. The selected
# Linux-musl compiler owns the C runtime and sysroot used by this ABI oracle.
compiler=${CC:-$default_compiler}
if ! command -v "$compiler" >/dev/null 2>&1 && [[ ! -x "$compiler" ]]; then
    echo "Linux-musl compiler '$compiler' is unavailable; set CC explicitly" >&2
    exit 2
fi
output_dir="out/$ANEMONE_ARCH"
mkdir -p "$output_dir"
compiler_path=$(command -v "$compiler" 2>/dev/null || realpath "$compiler")
sysroot=$($compiler -print-sysroot)
libc_archive=$($compiler -print-file-name=libc.a)
printf 'UDPEXT-C-BUILD:compiler=%s\n' "$compiler_path"
"$compiler" --version | head -n 1
printf 'UDPEXT-C-BUILD:sysroot=%s\n' "$sysroot"
printf 'UDPEXT-C-BUILD:libc=%s\n' "$libc_archive"
sha256sum "$libc_archive"
"$compiler" -static -O2 -std=c11 -Wall -Wextra -Werror main.c -o "$output_dir/udp-extension-c"
file "$output_dir/udp-extension-c"
