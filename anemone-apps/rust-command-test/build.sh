#!/usr/bin/env bash
set -euo pipefail

case "$ANEMONE_ARCH" in
    riscv64)
        expected_anemone_target=riscv64-unknown-anemone-elf
        rust_target=riscv64gc-unknown-linux-musl
        linker=${CC:-riscv64-unknown-linux-musl-gcc}
        linker_env=CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_LINKER
        ;;
    loongarch64)
        expected_anemone_target=loongarch64-unknown-anemone-elf
        rust_target=loongarch64-unknown-linux-musl
        linker=${CC:-loongarch64-unknown-linux-musl-gcc}
        linker_env=CARGO_TARGET_LOONGARCH64_UNKNOWN_LINUX_MUSL_LINKER
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

# Rust std's ordinary Command path is the real consumer under test. Build std
# for a static Linux-musl target so the guest needs no dynamic loader.
output_dir="out/$ANEMONE_ARCH"
unwind_dir="$output_dir/link"
mkdir -p "$unwind_dir"
cp link/libunwind.ld "$unwind_dir/libunwind.a"
env \
    "$linker_env=$linker" \
    RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -C link-self-contained=no -L native=$PWD/$unwind_dir" \
    cargo build \
        -Z build-std=std \
        --target "$rust_target" \
        --release \
        "$@"
cp "target/$rust_target/release/rust-command-test" "$output_dir/rust-command-test"
