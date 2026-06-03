#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: run-user-test-la64.sh <rootfs-config> <sdcard-image> [log-file]

Runs the la64 test chain:
  1. switch kconfig to qemu-virt-la64 if needed
  2. build the rootfs with sudo
  3. stage the provided sdcard image as a temporary copy
  4. build the kernel
  5. launch QEMU and tee the output to a log file
EOF
}

if [[ $# -lt 2 || $# -gt 3 ]]; then
    usage >&2
    exit 1
fi

rootfs_config=$1
sdcard_image=$2
log_file=${3:-build/user-test-la64.log}

platform=qemu-virt-la64
platform_arch=loongarch64
sdcard_target=sdcard-la.img
rootfs_target=build/rootfs/minimal-la/rootfs.img

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)
cd "$repo_root"


if [[ ! -f "$rootfs_config" ]]; then
    printf 'error: rootfs config not found: %s\n' "$rootfs_config" >&2
    exit 1
fi

if [[ ! -f "$sdcard_image" ]]; then
    printf 'error: sdcard image not found: %s\n' "$sdcard_image" >&2
    exit 1
fi

if [[ -e "$sdcard_target" ]]; then
    printf 'warning: %s already exists; will be overwritten\n' "$sdcard_target" >&2
fi

mkdir -p -- "$(dirname -- "$log_file")"

current_platform=$(
    awk -F'"' '/^[[:space:]]*platform[[:space:]]*=/ { print $2; exit }' kconfig
)

if [[ -z "$current_platform" ]]; then
    printf 'error: could not read current platform from kconfig\n' >&2
    exit 1
fi

printf 'test-chain: platform %s (%s)\n' "$platform" "$platform_arch"
printf 'test-chain: rootfs config %s\n' "$rootfs_config"
printf 'test-chain: sdcard image %s\n' "$sdcard_image"
printf 'test-chain: log file %s\n' "$log_file"

if [[ "$current_platform" != "$platform" ]]; then
    printf 'test-chain: switching kconfig from %s to %s\n' "$current_platform" "$platform"
    just conf switch "$platform"
fi

printf 'test-chain: rebuilding rootfs\n'
rm -rf -- build/rootfs
just rootfs mkfs -c "$rootfs_config" --sudo

mapfile -t rootfs_images < <(find build/rootfs -mindepth 2 -maxdepth 2 -type f -name rootfs.img | sort)
if [[ ${#rootfs_images[@]} -ne 1 ]]; then
    printf 'error: expected exactly one rootfs image, found %d\n' "${#rootfs_images[@]}" >&2
    if [[ ${#rootfs_images[@]} -gt 0 ]]; then
        printf 'error: candidates:\n' >&2
        printf 'error:   %s\n' "${rootfs_images[@]}" >&2
    fi
    exit 1
fi

rootfs_image=${rootfs_images[0]}
if [[ "$rootfs_image" != "$rootfs_target" ]]; then
    mkdir -p -- "$(dirname -- "$rootfs_target")"
    ln -sf -- "$rootfs_image" "$rootfs_target"
fi

printf 'test-chain: staging sdcard image\n'
cp -- "$sdcard_image" "$sdcard_target"

printf 'test-chain: building kernel\n'
just build

printf 'test-chain: running qemu\n'
just xtask qemu --platform "$platform" --image build/anemone.elf 2>&1 | tee "$log_file"
