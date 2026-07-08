#!/usr/bin/env bash
set -euo pipefail

log_progress() {
    local topic=$1
    local msg=$2
    printf '\033[1;35m%12s\033[0m %s\n' "$topic" "$msg"
}

warn() {
    local msg=$1
    printf '\033[1;33m%12s\033[0m %s\n' "WARNING" "$msg"
}

error() {
    local msg=$1
    printf '\033[1;31m%12s\033[0m %s\n' "ERROR" "$msg" >&2
}

usage() {
    cat <<'EOF'
Usage: run-user-test-la64.sh <sdcard-image> [log-file]

Runs the la64 test chain:
  1. switch kconfig to qemu-virt-la64-pretest if needed
  2. build the rootfs with sudo
  3. stage the provided sdcard image as a temporary copy
  4. build the kernel
  5. launch QEMU and tee the output to a log file

Uses conf/rootfs/pretest-la64.toml as the public pretest rootfs manifest.
EOF
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
    usage >&2
    exit 1
fi

rootfs_config=conf/rootfs/pretest-la64.toml
sdcard_image=$1
log_file=${2:-build/user-test-la64.log}

platform=qemu-virt-la64-pretest
platform_arch=loongarch64
sdcard_target=sdcard-la.img
rootfs_target=build/rootfs/pretest-la64/rootfs.img

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)
cd "$repo_root"


if [[ ! -f "$rootfs_config" ]]; then
    error "rootfs config not found: $rootfs_config"
    exit 1
fi

if [[ ! -f "$sdcard_image" ]]; then
    error "sdcard image not found: $sdcard_image"
    exit 1
fi

if [[ -e "$sdcard_target" ]]; then
    warn "$sdcard_target already exists; will be overwritten"
fi

mkdir -p -- "$(dirname -- "$log_file")"

current_platform=$(
    awk -F'"' '/^[[:space:]]*platform[[:space:]]*=/ { print $2; exit }' kconfig
)

if [[ -z "$current_platform" ]]; then
    error "could not read current platform from kconfig"
    exit 1
fi

log_progress "PRETEST" "platform $platform ($platform_arch)"
log_progress "PRETEST" "rootfs config $rootfs_config"
log_progress "PRETEST" "sdcard image $sdcard_image"
log_progress "PRETEST" "log file $log_file"

if [[ "$current_platform" != "$platform" ]]; then
    log_progress "PRETEST" "switching kconfig from $current_platform to $platform"
    just conf switch "$platform"
fi

log_progress "PRETEST" "rebuilding rootfs"
rm -rf -- build/rootfs
just rootfs mkfs -c "$rootfs_config" --sudo

mapfile -t rootfs_images < <(find build/rootfs -mindepth 2 -maxdepth 2 -type f -name rootfs.img | sort)
if [[ ${#rootfs_images[@]} -ne 1 ]]; then
    error "expected exactly one rootfs image, found ${#rootfs_images[@]}"
    if [[ ${#rootfs_images[@]} -gt 0 ]]; then
        error "candidates:"
        for rootfs_candidate in "${rootfs_images[@]}"; do
            error "  $rootfs_candidate"
        done
    fi
    exit 1
fi

rootfs_image=${rootfs_images[0]}
if [[ "$rootfs_image" != "$rootfs_target" ]]; then
    mkdir -p -- "$(dirname -- "$rootfs_target")"
    ln -sf -- "$rootfs_image" "$rootfs_target"
fi

log_progress "PRETEST" "staging sdcard image"
cp -- "$sdcard_image" "$sdcard_target"

log_progress "PRETEST" "building kernel"
just build

log_progress "PRETEST" "running qemu"
just xtask qemu --platform "$platform" --image build/anemone.elf 2>&1 | tee "$log_file"
