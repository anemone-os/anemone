#!/usr/bin/env bash
set -euo pipefail

case "$ANEMONE_ARCH" in
    riscv64)
        expected_anemone_target=riscv64-unknown-anemone-elf
        default_glibc_compiler=riscv64-linux-gnu-gcc
        default_musl_compiler=riscv64-linux-musl-gcc
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
    compiler=${HOST_CC:-$default_host_compiler}
    output_dir=out/host
    mkdir -p "$output_dir"
    printf 'UDP-ERRQUEUE-BUILD:family=host:compiler=%s\n' "$(command -v "$compiler")"
    "$compiler" --version | head -n 1
    "$compiler" -O2 -std=c11 -Wall -Wextra -Werror main.c -o "$output_dir/udp-errqueue-host"
    file "$output_dir/udp-errqueue-host"
    sha256sum "$output_dir/udp-errqueue-host"
    exit 0
fi

if [[ "$ANEMONE_TARGET_TRIPLE" != "$expected_anemone_target" ]]; then
    echo "ANEMONE_TARGET_TRIPLE '$ANEMONE_TARGET_TRIPLE' does not match '$expected_anemone_target'" >&2
    exit 2
fi

output_dir="out/$ANEMONE_ARCH"
mkdir -p "$output_dir"

build_oracle() {
    local family=$1 compiler=$2 output=$3
    local compiler_path libc_archive sysroot
    compiler_path=$(command -v "$compiler" 2>/dev/null || realpath "$compiler")
    sysroot=$($compiler -print-sysroot)
    libc_archive=$($compiler -print-file-name=libc.a)
    printf 'UDP-ERRQUEUE-BUILD:family=%s:compiler=%s\n' "$family" "$compiler_path"
    "$compiler" --version | head -n 1
    printf 'UDP-ERRQUEUE-BUILD:family=%s:sysroot=%s\n' "$family" "$sysroot"
    printf 'UDP-ERRQUEUE-BUILD:family=%s:libc=%s\n' "$family" "$libc_archive"
    sha256sum "$libc_archive"
    "$compiler" -static -O2 -std=c11 -Wall -Wextra -Werror main.c -o "$output"
    file "$output"
    sha256sum "$output"
}

build_oracle glibc "${GLIBC_CC:-$default_glibc_compiler}" "$output_dir/udp-errqueue-glibc"
build_oracle musl "${MUSL_CC:-$default_musl_compiler}" "$output_dir/udp-errqueue-musl"
