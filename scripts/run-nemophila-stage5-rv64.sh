#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: run-nemophila-stage5-rv64.sh <preliminary-sdcard-image> [log-file]" >&2
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
    usage
    exit 1
fi

master_disk=$1
log_file=${2:-build/nemophila-stage5-rv64.log}
rootfs_config=conf/rootfs/pretest-rv64.toml
rootfs_image=build/rootfs/pretest-rv64/rootfs.img
runtime_dir=build/runtime/nemophila-stage5-rv64
runtime_disk=$runtime_dir/disk-x0.img
preset=nemophila-clone-validation-rv64-release
artifact=build/modules/clone-observer/nemophila_clone_observer.wasm

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

if [[ ! -f $master_disk ]]; then
    echo "Nemophila Stage 5 master disk not found: $master_disk" >&2
    exit 1
fi
for runtime_parent in build build/runtime "$runtime_dir"; do
    if [[ -L $runtime_parent ]]; then
        echo "Nemophila Stage 5 runtime path must not be a symlink: $runtime_parent" >&2
        exit 1
    fi
done
mkdir -p -- "$runtime_dir" "$(dirname -- "$log_file")"

echo "NEMOPHILA-STAGE5:RUNNER:SOURCE:$(git rev-parse HEAD)"
echo "NEMOPHILA-STAGE5:RUNNER:DISK:$(sha256sum "$master_disk" | cut -d' ' -f1)"

just module build clone-observer
if [[ ! -f $artifact || -L $artifact ]]; then
    echo "Nemophila Stage 5 fresh ordinary artifact missing: $artifact" >&2
    exit 1
fi

just rootfs mkfs -c "$rootfs_config"
cp --remove-destination -- "$master_disk" "$runtime_disk"
just build --preset "$preset"

set +e
timeout --foreground 600 just qemu --preset "$preset" \
    --bind smp=1 --bind memory=1G \
    --bind kernel-image=build/anemone.elf \
    --bind disk-x0="$runtime_disk" \
    --bind disk-x1="$rootfs_image" 2>&1 | tee "$log_file"
qemu_status=${PIPESTATUS[0]}
set -e

if [[ $qemu_status -ne 0 ]]; then
    echo "Nemophila Stage 5 QEMU failed with status $qemu_status" >&2
    exit "$qemu_status"
fi

require_marker() {
    local marker=$1
    grep -Fq "$marker" "$log_file" || {
        echo "Nemophila Stage 5 marker missing: $marker" >&2
        exit 1
    }
}

require_count() {
    local marker=$1
    local expected=$2
    local actual
    # printk records can be adjacent on one serial line, so count fixed-string
    # occurrences rather than matching lines.
    actual=$(grep -Fo "$marker" "$log_file" | wc -l)
    if [[ $actual -ne $expected ]]; then
        echo "Nemophila Stage 5 marker count mismatch: expected=$expected actual=$actual marker=$marker" >&2
        exit 1
    fi
}

require_marker "NEMOPHILA-STAGE5:ACTIVATE:START:artifact-bytes="
require_marker "NEMOPHILA-STAGE5:CANONICAL:INITIAL-LOAD:"
require_marker "NEMOPHILA-STAGE5:CANONICAL:INITIAL-UNLOAD:"
require_marker "NEMOPHILA-STAGE5:CANONICAL:RELOAD:"
require_marker "NEMOPHILA-STAGE5:CANONICAL:SECOND-LOAD:"
require_marker "NEMOPHILA-STAGE5:SYNTHETIC:TRAP-DISPATCHED"
require_marker "NEMOPHILA-STAGE5:SYNTHETIC:POISONED-UNLOAD:"
require_marker "NEMOPHILA-STAGE5:SYNTHETIC:TRAP-RELOAD:"
require_marker "NEMOPHILA-STAGE5:ACTIVATE:READY:"
require_count "duplicate clone registration rejected; callback environment released" 3
require_count "clone creator=4294967294 child=4294967295" 2

init_child=$(sed -n 's/.*init: forked child process with tid \([0-9][0-9]*\).*/\1/p' "$log_file" | tail -n 1)
if [[ -z $init_child ]]; then
    echo "Nemophila Stage 5 could not resolve init clone child TID" >&2
    exit 1
fi
require_count "clone creator=1 child=$init_child" 2

clone_line=$(grep -F "NEMOPHILA-STAGE5:CLONE:RETURN:" "$log_file" | tail -n 1)
clone_creator=$(sed -n 's/.*:creator=\([0-9][0-9]*\):child=.*/\1/p' <<<"$clone_line")
clone_child=$(sed -n 's/.*:child=\([0-9][0-9]*\).*/\1/p' <<<"$clone_line")
if [[ -z $clone_creator || -z $clone_child ]]; then
    echo "Nemophila Stage 5 could not resolve clone oracle TIDs" >&2
    exit 1
fi
require_count "clone creator=$clone_creator child=$clone_child" 2
require_marker "NEMOPHILA-STAGE5:CLONE:COMPLETE:child=$clone_child"

clone3_line=$(grep -F "NEMOPHILA-STAGE5:CLONE3:RETURN:" "$log_file" | tail -n 1)
clone3_creator=$(sed -n 's/.*:creator=\([0-9][0-9]*\):child=.*/\1/p' <<<"$clone3_line")
clone3_child=$(sed -n 's/.*:child=\([0-9][0-9]*\).*/\1/p' <<<"$clone3_line")
if [[ -z $clone3_creator || -z $clone3_child ]]; then
    echo "Nemophila Stage 5 could not resolve clone3 oracle TIDs" >&2
    exit 1
fi
require_count "clone creator=$clone3_creator child=$clone3_child" 2

# The orderly shutdown can begin after the final userspace writes have
# returned but before those bytes drain to the serial log. Prefer the explicit
# userspace completion marker; if it was lost at that boundary, require the
# stronger kernel-side proof that the parent matched and retired this exact
# clone3 child before PowerOff.
if ! grep -Fq "NEMOPHILA-STAGE5:CLONE3:COMPLETE:child=$clone3_child" "$log_file"; then
    require_marker "wait: found a child task #$clone3_child that satisfies the wait condition"
    require_marker "thread group task #$clone3_child is dropped"
fi

# One warning belongs to the explicitly synthetic cycle and one to init's real
# clone. No further warning is permitted: the poisoned instance must not enter
# either focused user-test cohort.
require_count "Nemophila callback poisoned instance=" 2
require_marker "system-power: executor core #0 entering PowerOff machine action"
echo "NEMOPHILA-STAGE5:RUNNER:SUMMARY:PASS:arch=rv64"
