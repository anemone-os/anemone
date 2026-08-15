#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: run-net-tcp-stage5.sh <rv64|la64> <validation-disk> [log-file]" >&2
}

if [[ $# -lt 2 || $# -gt 3 ]]; then
    usage
    exit 1
fi

arch=$1
validation_disk=$2
case $arch in
    rv64)
        rootfs_config=conf/rootfs/pretest-rv64.toml
        rootfs_image=build/rootfs/pretest-rv64/rootfs.img
        target=net-tcp-stage5-rv64
        rootfs_slot=disk-x1
        validation_slot=disk-x0
        net_bind=()
        ;;
    la64)
        rootfs_config=conf/rootfs/pretest-la64.toml
        rootfs_image=build/rootfs/pretest-la64/rootfs.img
        target=net-tcp-stage5-la64
        rootfs_slot=disk-x0
        validation_slot=disk-x1
        net_bind=(
            --bind 'net-user-options=restrict=off,hostfwd=tcp::26010-:26010,hostfwd=tcp::26011-:26011,hostfwd=tcp::26012-:26012'
        )
        ;;
    *)
        usage
        exit 1
        ;;
esac

log_file=${3:-build/net-tcp-stage5-${arch}.log}
peer_log=${log_file%.log}-peer.log
ingress_peer_log=${log_file%.log}-ingress-peer.log
runtime_dir=build/runtime/net-tcp-stage5-${arch}
runtime_disk=$runtime_dir/validation.img
peer_port=25050
peer_sessions=4
ingress_peer_port=25051
ingress_peer_sessions=2
peer_pid=
ingress_peer_pid=

cleanup() {
    local status=$?
    trap - EXIT INT TERM
    if [[ -n $peer_pid ]] && kill -0 "$peer_pid" 2>/dev/null; then
        kill "$peer_pid" 2>/dev/null || true
        wait "$peer_pid" 2>/dev/null || true
    fi
    if [[ -n $ingress_peer_pid ]] && kill -0 "$ingress_peer_pid" 2>/dev/null; then
        kill "$ingress_peer_pid" 2>/dev/null || true
        wait "$ingress_peer_pid" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"
if [[ ! -f $validation_disk ]]; then
    echo "validation disk not found: $validation_disk" >&2
    exit 1
fi
mkdir -p "$runtime_dir" "$(dirname -- "$log_file")"

echo "TCPSTAGE5:RUNNER:SOURCE:$(git rev-parse HEAD)"
echo "TCPSTAGE5:RUNNER:DISK:$(sha256sum "$validation_disk" | cut -d' ' -f1)"
just rootfs mkfs -c "$rootfs_config"
cp --remove-destination -- "$validation_disk" "$runtime_disk"
just build --target "$target" --kernel-config conf/kconfs/default.toml --profile release

python3 scripts/net-tcp-stage5-peer.py \
    --port "$peer_port" --sessions "$peer_sessions" >"$peer_log" 2>&1 &
peer_pid=$!
for _ in {1..100}; do
    if grep -Fq 'TCPSTAGE5:PEER:READY' "$peer_log" 2>/dev/null; then
        break
    fi
    if ! kill -0 "$peer_pid" 2>/dev/null; then
        cat "$peer_log" >&2
        exit 1
    fi
    sleep 0.1
done
grep -Fq 'TCPSTAGE5:PEER:READY' "$peer_log"

python3 scripts/net-tcp-listener-peer.py \
    --control-port "$ingress_peer_port" --sessions "$ingress_peer_sessions" \
    >"$ingress_peer_log" 2>&1 &
ingress_peer_pid=$!
for _ in {1..100}; do
    if grep -Fq 'TCPINGRESS:PEER:READY' "$ingress_peer_log" 2>/dev/null; then
        break
    fi
    if ! kill -0 "$ingress_peer_pid" 2>/dev/null; then
        cat "$ingress_peer_log" >&2
        exit 1
    fi
    sleep 0.1
done
grep -Fq 'TCPINGRESS:PEER:READY' "$ingress_peer_log"

disk_bind=(--bind "$rootfs_slot=$rootfs_image" --bind "$validation_slot=$runtime_disk")

set +e
if [[ $arch == rv64 ]]; then
    # Validation-only bridge: the RV64 Platform currently owns a fixed
    # `-netdev user,id=net` surface without an options bind. Inject hostfwd via
    # HMP instead of changing that production Platform contract; remove this
    # branch once RV64 gains an owner-approved backend-options bind.
    {
        sleep 2
        printf '\001c'
        sleep 0.1
        printf 'hostfwd_add net tcp::26010-:26010\n'
        printf 'hostfwd_add net tcp::26011-:26011\n'
        printf 'hostfwd_add net tcp::26012-:26012\n'
        sleep 0.1
        printf '\001c'
    } | timeout --foreground 600 just qemu \
        --target "$target" --kernel-config conf/kconfs/default.toml --profile release \
        --bind smp=1 --bind memory=1G \
        --bind kernel-image=build/anemone.elf \
        "${disk_bind[@]}" 2>&1 | tee "$log_file"
    qemu_status=${PIPESTATUS[1]}
else
    timeout --foreground 600 just qemu \
        --target "$target" --kernel-config conf/kconfs/default.toml --profile release \
        --bind smp=1 --bind memory=1G \
        "${net_bind[@]}" \
        --bind kernel-image=build/anemone.elf \
        "${disk_bind[@]}" 2>&1 | tee "$log_file"
    qemu_status=${PIPESTATUS[0]}
fi
set -e

wait "$peer_pid"
peer_pid=
cat "$peer_log"
wait "$ingress_peer_pid"
ingress_peer_pid=
cat "$ingress_peer_log"

[[ $(grep -Fc 'TPASS: tcp_r0_self_external' "$log_file") -eq 2 ]]
[[ $(grep -Fc 'TPASS: tcp_r0_listener_ingress' "$log_file") -eq 2 ]]
[[ $(grep -Fc 'TPASS: tcp_r0_remote_external' "$log_file") -eq 2 ]]
[[ $(grep -Fc 'TPASS: tcp_r0_remote_reset' "$log_file") -eq 2 ]]
grep -Fq "TCPSTAGE5:CAGENT:CLIENTS:PASS:2" "$log_file"
grep -Fq "TCPSTAGE5:CAGENT:CLEANUP:PASS" "$log_file"
grep -Fq 'TCPSTAGE5:SOCK-DIAG:PASS:idiag_if=0' "$log_file"
grep -Fq "TCPSTAGE5:SS:PASS:arch=${arch}:libc=glibc" "$log_file"
grep -Fq "TCPSTAGE5:CAGENT:SUMMARY:PASS:arch=${arch}:libc=glibc" "$log_file"
grep -Fq "TCPSTAGE5:SUMMARY:PASS:arch=${arch}" "$log_file"
grep -Eq 'LTP whitelist finished: attempted=[0-9]+ passed=[0-9]+ failed=0 infra_failed=0' "$log_file"
grep -Fq 'user-test: all tests finished, shutting down.' "$log_file"
[[ $(grep -Fc 'kind=stream' "$peer_log") -eq 2 ]]
[[ $(grep -Fc 'kind=reset' "$peer_log") -eq 2 ]]
grep -Fq 'TCPSTAGE5:PEER:SUMMARY:PASS:4' "$peer_log"
[[ $(grep -Fc 'TCPINGRESS:PEER:PHASE:PASS' "$ingress_peer_log") -eq 6 ]]
grep -Fq 'TCPINGRESS:PEER:SUMMARY:PASS:2' "$ingress_peer_log"

if [[ $qemu_status -ne 0 && ! ( $arch == la64 && $qemu_status -eq 124 ) ]]; then
    echo "QEMU failed with status $qemu_status" >&2
    exit "$qemu_status"
fi
if [[ $qemu_status -eq 124 ]]; then
    echo "TCPSTAGE5:RUNNER:TERMINAL:launcher-timeout-after-complete-markers"
else
    echo "TCPSTAGE5:RUNNER:TERMINAL:qemu-exit-0"
fi
echo "TCPSTAGE5:RUNNER:SUMMARY:PASS:arch=${arch}"
