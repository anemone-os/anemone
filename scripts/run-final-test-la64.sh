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
Usage: run-final-test-la64.sh <disk-image> [log-file]

Builds a la64 kernel with the fixed embedded BusyBox shell, stages a writable
copy of the provided disk image, and launches the generic 8-CPU/8-GiB QEMU VM.
EOF
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
    usage >&2
    exit 1
fi

disk_image=$1
log_file=${2:-build/final-test-la64.log}
runtime_dir=build/runtime/final-la64
disk_target=$runtime_dir/disk-x0.img
selection=(
    --target qemu-virt-la64-final
    --kernel-config conf/.defconfig
    --profile release
)
provider_bindings=(--bind smp=8 --bind memory=8G)

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)
cd "$repo_root"

if [[ ! -f "$disk_image" ]]; then
    error "disk image not found: $disk_image"
    exit 1
fi

for runtime_parent in build build/runtime "$runtime_dir"; do
    if [[ -L "$runtime_parent" ]]; then
        error "runtime directory component must not be a symlink: $runtime_parent"
        exit 1
    fi
done

if [[ -e "$disk_target" || -L "$disk_target" ]]; then
    warn "$disk_target already exists; will be overwritten"
fi

mkdir -p -- "$(dirname -- "$log_file")" "$runtime_dir"

log_progress "FINAL" "target qemu-virt-la64-final (loongarch64)"
log_progress "FINAL" "topology smp=8 memory=8G"
log_progress "FINAL" "disk image $disk_image"
log_progress "FINAL" "log file $log_file"

disk_source=$(realpath -- "$disk_image")
disk_destination=$(realpath -m -- "$disk_target")
if [[ "$disk_source" == "$disk_destination" ]]; then
    error "disk source and runtime destination must be different files"
    exit 1
fi

log_progress "FINAL" "staging disk image"
cp --remove-destination -- "$disk_image" "$disk_target"

log_progress "FINAL" "building kernel"
just build "${selection[@]}" "${provider_bindings[@]}"

log_progress "FINAL" "running qemu"
just qemu "${selection[@]}" "${provider_bindings[@]}" \
    --bind kernel-image=build/anemone.elf \
    --bind disk-x0="$disk_target" 2>&1 | tee "$log_file"
