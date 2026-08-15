#!/usr/bin/env bash
set -euo pipefail

readonly timeout_seconds=20
readonly module_path=build/modules/clone-observer/nemophila_clone_observer.wasm

fail() {
    printf 'nemophila-r0 validation: %s\n' "$1" >&2
    exit 1
}

run_qemu() {
    local arch=$1
    local preset=$2
    local rootfs=${3:-}
    local log=$4
    local -a bindings=(
        --bind smp=2
        --bind memory=1G
        --bind kernel-image=build/anemone.elf
    )
    if [[ $arch == la64 ]]; then
        bindings+=(--bind net-user-options=restrict=off)
    fi
    if [[ -n $rootfs ]]; then
        bindings+=(--bind "disk-x0=$rootfs")
    fi

    set +e
    timeout --signal=INT "${timeout_seconds}s" \
        just qemu --preset "$preset" "${bindings[@]}" 2>&1 | tee "$log"
    local status=${PIPESTATUS[0]}
    set -e

    if [[ $arch == rv64 ]]; then
        [[ $status -eq 0 ]] || fail "$preset QEMU exited with status $status"
    else
        # LA64 currently reaches the kernel's terminal halt because its QEMU
        # machine has no successful poweroff handler. The timeout is only the
        # host failure bound; the guest marker below is the semantic oracle.
        [[ $status -eq 0 || $status -eq 124 ]] \
            || fail "$preset QEMU exited with status $status"
    fi
}

run_positive() {
    local arch=$1
    local preset="nemophila-r0-validation-${arch}-release"
    local rootfs_config="conf/rootfs/nemophila-r0-validation-${arch}.toml"
    local rootfs="build/rootfs/nemophila-r0-validation-${arch}/rootfs.img"
    local log="build/nemophila-r0-validation-${arch}.log"

    just build --preset "$preset"
    [[ -f $module_path ]] || fail "system build did not export $module_path"
    local hash
    hash=$(sha256sum "$module_path" | awk '{print $1}')
    printf 'nemophila-r0 validation: %s module sha256=%s\n' "$arch" "$hash"
    just rootfs mkfs -c "$rootfs_config"
    [[ -f $rootfs ]] || fail "rootfs build did not produce $rootfs"
    run_qemu "$arch" "$preset" "$rootfs" "$log"

    # SMP printk may interleave bytes with intermediate userspace lines. The
    # final marker is emitted only after both validation groups return success.
    rg -q 'NEMOPHILA-R0:VALIDATION:PASS' "$log" \
        || fail "$arch validation marker missing"
    printf '%s\n' "$hash"
}

run_negative() {
    local arch=$1
    local preset="nemophila-r0-boot-negative-${arch}-release"
    local log="build/nemophila-r0-boot-negative-${arch}.log"

    just build --preset "$preset"
    run_qemu "$arch" "$preset" '' "$log"

    rg -q 'Nemophila boot load begin identity=boot-reject-validation ordinal=0 phase=load' "$log" \
        || fail "$arch boot-negative begin marker missing"
    rg -q 'Nemophila boot load failed identity=boot-reject-validation ordinal=0 phase=load-and-publish error=Load' "$log" \
        || fail "$arch boot-negative failure marker missing"
    rg -q 'required Nemophila module failed identity=boot-reject-validation ordinal=0 phase=load-and-publish' "$log" \
        || fail "$arch boot-negative panic marker missing"
    if rg -q 'NEMOPHILA-R0:NEGATIVE:INITIAL-USERSPACE-REACHED' "$log"; then
        fail "$arch boot-negative target reached initial userspace"
    fi
}

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

rv64_hash=$(run_positive rv64 | tee /dev/stderr | tail -n 1)
la64_hash=$(run_positive la64 | tee /dev/stderr | tail -n 1)
[[ $rv64_hash == "$la64_hash" ]] \
    || fail "fresh RV64/LA64 module hashes differ: $rv64_hash != $la64_hash"

run_negative rv64
run_negative la64
printf 'NEMOPHILA-R0:HOST-VALIDATION:PASS sha256=%s\n' "$rv64_hash"
