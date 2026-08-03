#!/usr/bin/env bash
set -euo pipefail

case "$ANEMONE_ARCH" in
    riscv64)
        expected_anemone_target=riscv64-unknown-anemone-elf
        zig_target=riscv64-linux-musl
        ;;
    loongarch64)
        expected_anemone_target=loongarch64-unknown-anemone-elf
        zig_target=loongarch64-linux-musl
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

# Zig owns its Linux-musl runtime target. ANEMONE_TARGET_TRIPLE identifies the
# exported artifact and is not passed to Zig as a compiler target.
output_dir="out/$ANEMONE_ARCH"
mkdir -p "$output_dir"
zig build-exe \
    -target "$zig_target" \
    -O ReleaseSmall \
    -static \
    -lc \
    "$@" \
    main.zig \
    -femit-bin="$output_dir/zig-hello"
