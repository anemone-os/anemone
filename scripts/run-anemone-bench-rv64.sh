#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: run-anemone-bench-rv64.sh [--rootfs-sudo] [log-file]

Builds and boots the dedicated single-core RV64 benchmark rootfs. At the shell,
run /bin/anemone-bench directly or through /bin/perfctl, then use /bin/shutdown.
EOF
}

rootfs_args=(-c conf/rootfs/anemone-bench-rv64.toml)
if [[ ${1:-} == -h || ${1:-} == --help ]]; then
    usage
    exit 0
fi
if [[ ${1:-} == --rootfs-sudo ]]; then
    rootfs_args+=(--sudo)
    shift
fi
if [[ ${1:-} == -h || ${1:-} == --help ]]; then
    usage
    exit 0
fi
if [[ $# -gt 1 ]]; then
    usage >&2
    exit 2
fi

log_file=${1:-build/anemone-bench-rv64.log}
rootfs_image=build/rootfs/anemone-bench-rv64/rootfs.img
preset=qemu-virt-rv64-bench-release
provider_bindings=(--bind smp=1 --bind memory=1G)

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)
cd "$repo_root"
mkdir -p -- "$(dirname -- "$log_file")"

printf 'ANEMONE-BENCH: rebuilding %s\n' "$rootfs_image"
just rootfs mkfs "${rootfs_args[@]}"
printf 'ANEMONE-BENCH: building %s with smp=1 memory=1G\n' "$preset"
just build --preset "$preset" "${provider_bindings[@]}"
printf 'ANEMONE-BENCH: booting; log=%s\n' "$log_file"
just qemu --preset "$preset" "${provider_bindings[@]}" \
    --bind kernel-image=build/anemone.elf \
    --bind disk-x0="$rootfs_image" 2>&1 | tee "$log_file"
