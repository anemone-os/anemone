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
  1. build the rootfs with sudo
  2. stage the provided sdcard image as a build-local temporary copy
  3. explicitly select the pretest preset and build the kernel
  4. launch QEMU with the complete tracked bind map and tee the output to a log file

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

preset=qemu-virt-la64-pretest-release
target_arch=loongarch64
runtime_dir=build/runtime/pretest-la64
sdcard_target=$runtime_dir/disk-x1.img
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

for runtime_parent in build build/runtime "$runtime_dir"; do
    if [[ -L "$runtime_parent" ]]; then
        error "runtime directory component must not be a symlink: $runtime_parent"
        exit 1
    fi
done

if [[ -e "$sdcard_target" || -L "$sdcard_target" ]]; then
    warn "$sdcard_target already exists; will be overwritten"
fi

mkdir -p -- "$(dirname -- "$log_file")"

log_progress "PRETEST" "preset $preset ($target_arch)"
log_progress "PRETEST" "rootfs config $rootfs_config"
log_progress "PRETEST" "sdcard image $sdcard_image"
log_progress "PRETEST" "log file $log_file"

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
    error "unexpected rootfs output: $rootfs_image (expected $rootfs_target)"
    exit 1
fi

log_progress "PRETEST" "staging sdcard image"
mkdir -p -- "$runtime_dir"
sdcard_source=$(realpath -- "$sdcard_image")
sdcard_destination=$(realpath -m -- "$sdcard_target")
if [[ "$sdcard_source" == "$sdcard_destination" ]]; then
    error "sdcard source and runtime destination must be different files"
    exit 1
fi
cp --remove-destination -- "$sdcard_image" "$sdcard_target"

log_progress "PRETEST" "building kernel"
just build --preset "$preset"

log_progress "PRETEST" "running qemu"
just qemu --preset "$preset" \
    --bind kernel-image=build/anemone.elf \
    --bind disk-x0="$rootfs_target" \
    --bind disk-x1="$sdcard_target" 2>&1 | tee "$log_file"
