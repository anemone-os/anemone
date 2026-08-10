#!/usr/bin/env bash
set -euo pipefail

case "$ANEMONE_ARCH" in
    riscv64)
        expected_anemone_target=riscv64-unknown-anemone-elf
        default_glibc_compiler=riscv64-linux-gnu-gcc
        default_musl_compiler=riscv64-linux-musl-gcc
        ;;
    loongarch64)
        expected_anemone_target=loongarch64-unknown-anemone-elf
        default_glibc_compiler=loongarch64-unknown-linux-gnu-gcc
        default_musl_compiler=loongarch64-unknown-linux-musl-gcc
        ;;
    host)
        expected_anemone_target=
        default_host_compiler=cc
        ;;
    *)
        echo "unsupported ANEMONE_ARCH: $ANEMONE_ARCH" >&2
        exit 2
        ;;
esac

if [[ "$ANEMONE_ARCH" == host ]]; then
    if [[ -n ${ANEMONE_TARGET_TRIPLE+x} ]]; then
        echo "host build must not inherit ANEMONE_TARGET_TRIPLE" >&2
        exit 2
    fi
    host_compiler=${HOST_CC:-$default_host_compiler}
    if ! command -v "$host_compiler" >/dev/null 2>&1 && [[ ! -x "$host_compiler" ]]; then
        echo "host compiler '$host_compiler' is unavailable; set HOST_CC explicitly" >&2
        exit 2
    fi
    output_dir=out/host
    mkdir -p "$output_dir"
    compiler_path=$(command -v "$host_compiler" 2>/dev/null || realpath "$host_compiler")
    printf 'SOCKET-R1-BUILD:family=host:compiler=%s\n' "$compiler_path"
    "$host_compiler" --version | head -n 1
    "$host_compiler" -O2 -std=c11 -Wall -Wextra -Werror main.c -o "$output_dir/socket-r1-host"
    file "$output_dir/socket-r1-host"
    sha256sum "$output_dir/socket-r1-host"
    exit 0
fi

if [[ "$ANEMONE_TARGET_TRIPLE" != "$expected_anemone_target" ]]; then
    echo "ANEMONE_TARGET_TRIPLE '$ANEMONE_TARGET_TRIPLE' does not match '$expected_anemone_target'" >&2
    exit 2
fi

glibc_compiler=${GLIBC_CC:-$default_glibc_compiler}
musl_compiler=${MUSL_CC:-$default_musl_compiler}
output_dir="out/$ANEMONE_ARCH"
mkdir -p "$output_dir"

build_oracle() {
    local family=$1
    local compiler=$2
    local output=$3

    if ! command -v "$compiler" >/dev/null 2>&1 && [[ ! -x "$compiler" ]]; then
        echo "$family compiler '$compiler' is unavailable; set ${family^^}_CC explicitly" >&2
        exit 2
    fi

    local compiler_path
    local libc_archive
    local sysroot
    compiler_path=$(command -v "$compiler" 2>/dev/null || realpath "$compiler")
    sysroot=$($compiler -print-sysroot)
    libc_archive=$($compiler -print-file-name=libc.a)
    printf 'SOCKET-R1-BUILD:family=%s:compiler=%s\n' "$family" "$compiler_path"
    "$compiler" --version | head -n 1
    printf 'SOCKET-R1-BUILD:family=%s:sysroot=%s\n' "$family" "$sysroot"
    printf 'SOCKET-R1-BUILD:family=%s:libc=%s\n' "$family" "$libc_archive"
    sha256sum "$libc_archive"
    "$compiler" -static -O2 -std=c11 -Wall -Wextra -Werror main.c -o "$output"
    file "$output"
    sha256sum "$output"
}

build_oracle glibc "$glibc_compiler" "$output_dir/socket-r1-glibc"
build_oracle musl "$musl_compiler" "$output_dir/socket-r1-musl"
