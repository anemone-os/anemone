#!/usr/bin/env bash
set -euo pipefail

readonly expected_busybox_sha256=fd9cb9dc66ba740dc94b055b564de0597453adfceef9be158b3774ca58b95241
readonly platform=qemu-virt-rv64-pretest
readonly rootfs_config=conf/rootfs/tty-acceptance-rv64.toml
readonly staging_dir=build/tty-acceptance/staging/riscv64
readonly staged_busybox=$staging_dir/busybox
readonly staged_mode=$staging_dir/mode
readonly acceptance_rootfs=build/rootfs/tty-acceptance-rv64/rootfs.img
readonly platform_rootfs=build/rootfs/pretest-rv64/rootfs.img
readonly sdcard_target=sdcard-rv.img

usage() {
    cat <<'EOF'
Usage: run-tty-test-rv64.sh --busybox PATH --sdcard PATH --mode auto|vi [--log PATH]

Builds and runs the Stage 2 RV64 TTY acceptance rootfs. External BusyBox and
sdcard inputs are validated and copied; the originals remain read-only.
EOF
}

fail() {
    printf 'TTY-HARNESS:FAIL:%s\n' "$*" >&2
    exit 1
}

progress() {
    printf 'TTY-HARNESS:%s\n' "$*"
}

busybox=
sdcard=
mode=
log_file=build/tty-stage2-rv64.log

while [[ $# -gt 0 ]]; do
    case $1 in
        --busybox)
            [[ $# -ge 2 ]] || fail "--busybox requires a path"
            busybox=$2
            shift 2
            ;;
        --sdcard)
            [[ $# -ge 2 ]] || fail "--sdcard requires a path"
            sdcard=$2
            shift 2
            ;;
        --mode)
            [[ $# -ge 2 ]] || fail "--mode requires auto or vi"
            mode=$2
            shift 2
            ;;
        --log)
            [[ $# -ge 2 ]] || fail "--log requires a path"
            log_file=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            fail "unknown argument: $1"
            ;;
    esac
done

[[ -n $busybox ]] || fail "--busybox is required"
[[ -n $sdcard ]] || fail "--sdcard is required"
[[ $mode == auto || $mode == vi ]] || fail "--mode must be auto or vi"
[[ -f $busybox ]] || fail "BusyBox not found: $busybox"
[[ -f $sdcard ]] || fail "sdcard master not found: $sdcard"

for command in file readelf sha256sum python3; do
    command -v "$command" >/dev/null || fail "required host command not found: $command"
done

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)
cd "$repo_root"

busybox_file=$(file -b -- "$busybox")
[[ $busybox_file == *RISC-V* ]] || fail "BusyBox is not a RISC-V ELF: $busybox_file"
[[ $busybox_file == *statically\ linked* ]] || fail "BusyBox is not statically linked: $busybox_file"
readelf -h -- "$busybox" | grep -Eq 'Machine:[[:space:]]+RISC-V' \
    || fail "readelf did not report RISC-V"

busybox_sha256=$(sha256sum -- "$busybox")
busybox_sha256=${busybox_sha256%% *}
[[ $busybox_sha256 == "$expected_busybox_sha256" ]] \
    || fail "BusyBox SHA-256 mismatch: $busybox_sha256"

if command -v qemu-riscv64 >/dev/null; then
    busybox_help=$(qemu-riscv64 "$busybox" --help 2>&1)
    [[ $busybox_help == *"BusyBox v1.33.1"* ]] || fail "expected BusyBox v1.33.1"
    busybox_applets=$(qemu-riscv64 "$busybox" --list)
    for applet in ash stty vi mount stat poweroff; do
        grep -qx -- "$applet" <<<"$busybox_applets" || fail "BusyBox applet missing: $applet"
    done
    busybox_runtime_check=host-qemu-riscv64
else
    # tty-test repeats the version/applet checks before any acceptance case.
    # This fallback keeps artifact identity fail-closed on hosts without qemu-user.
    busybox_runtime_check=guest-launcher
fi

mkdir -p -- "$staging_dir" "$(dirname -- "$log_file")" "$(dirname -- "$platform_rootfs")"
cp --preserve=mode -- "$busybox" "$staged_busybox"
printf '%s\n' "$mode" >"$staged_mode"

current_platform=$(awk -F'"' '/^[[:space:]]*platform[[:space:]]*=/ { print $2; exit }' kconfig)
[[ -n $current_platform ]] || fail "could not read platform from kconfig"
if [[ $current_platform != "$platform" ]]; then
    progress "switch-platform:$current_platform->$platform"
    just conf switch "$platform"
fi

if [[ -n $(git status --porcelain --untracked-files=all) ]]; then
    candidate_dirty=yes
else
    candidate_dirty=no
fi

{
    progress "base-commit:$(git rev-parse HEAD)"
    progress "candidate-dirty:$candidate_dirty"
    progress "platform:$platform"
    progress "mode:$mode"
    progress "busybox-sha256:$busybox_sha256"
    progress "busybox-version:1.33.1"
    progress "busybox-runtime-check:$busybox_runtime_check"
    progress "rootfs-config:$rootfs_config"
} | tee "$log_file"

progress "build-rootfs"
just rootfs mkfs -c "$rootfs_config" --sudo 2>&1 | tee -a "$log_file"
[[ -f $acceptance_rootfs ]] || fail "rootfs image not produced: $acceptance_rootfs"

# The pretest platform currently owns a fixed rootfs path. Keep this bridge local
# to the acceptance wrapper; remove it when the QEMU owner accepts an explicit
# rootfs image without changing the tracked platform configuration.
cp -- "$acceptance_rootfs" "$platform_rootfs"
cp -- "$sdcard" "$sdcard_target"
rootfs_sha256=$(sha256sum -- "$acceptance_rootfs")
rootfs_sha256=${rootfs_sha256%% *}
progress "rootfs-sha256:$rootfs_sha256" | tee -a "$log_file"

progress "build-kernel"
just build 2>&1 | tee -a "$log_file"
kernel_sha256=$(sha256sum -- build/anemone.elf)
kernel_sha256=${kernel_sha256%% *}
progress "kernel-sha256:$kernel_sha256" | tee -a "$log_file"

if [[ $mode == auto ]]; then
    progress "qemu-auto"
    python3 - "$log_file" <<'PY'
import os
import select
import signal
import subprocess
import sys
import time

log_path = sys.argv[1]
command = [
    "just", "xtask", "qemu",
    "--platform", "qemu-virt-rv64-pretest",
    "--image", "build/anemone.elf",
]
inputs = {
    b"@@TTY READY canonical-incomplete@@": b"abc",
    b"@@TTY READY canonical-newline@@": b"\n",
    b"@@TTY READY canonical-erase@@": b"ab\x7fc\n",
    b"@@TTY READY canonical-kill@@": b"abc\x15d\n",
    b"@@TTY READY canonical-eof@@": b"xy\x04",
    b"@@TTY READY canonical-empty-eof@@": b"\x04",
    b"@@TTY READY canonical-short-record@@": b"12345\nsecond\n",
    b"@@TTY READY icrnl@@": b"q\r",
    b"@@TTY READY noncanonical-vmin1-vtime0@@": b"\x00A",
    b"@@TTY READY tcsetsf-flush@@": b"dropme\n",
    b"@@TTY READY readiness@@": b"ready\n",
}
vi_seed = b"TTYVI-SEED-71C4"
vi_ready = b"@@TTY VI raw-ready@@"
vi_keys = (b"G", b"o", b"alpha\n", b"beta", b"\x1b", b":wq\n")
sent = set()
seen = bytearray()
deadline = time.monotonic() + 240

with open(log_path, "ab", buffering=0) as log:
    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    assert proc.stdin is not None and proc.stdout is not None
    try:
        while True:
            if time.monotonic() >= deadline:
                raise TimeoutError("QEMU auto matrix exceeded 240 seconds")
            readable, _, _ = select.select([proc.stdout], [], [], 1.0)
            if readable:
                chunk = os.read(proc.stdout.fileno(), 4096)
                if chunk:
                    log.write(chunk)
                    sys.stdout.buffer.write(chunk)
                    sys.stdout.buffer.flush()
                    seen.extend(chunk)
                    for marker, payload in inputs.items():
                        if marker in seen and marker not in sent:
                            proc.stdin.write(payload)
                            proc.stdin.flush()
                            sent.add(marker)
                    if vi_ready in seen and vi_ready not in sent:
                        # Keep ESC separate from the following command. BusyBox vi
                        # distinguishes a standalone mode switch from an escape
                        # sequence using arrival timing while the terminal is raw.
                        for keys in vi_keys:
                            proc.stdin.write(keys)
                            proc.stdin.flush()
                            time.sleep(0.05)
                        sent.add(vi_ready)
                elif proc.poll() is not None:
                    break
            elif proc.poll() is not None:
                break
        returncode = proc.wait(timeout=5)
    except BaseException:
        try:
            proc.stdin.write(b"\x01x")
            proc.stdin.flush()
        except BaseException:
            pass
        os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait(timeout=5)
        raise

data = bytes(seen)
missing = [marker.decode("ascii") for marker in inputs if marker not in sent]
if vi_ready not in sent:
    missing.append(vi_ready.decode("ascii"))
if missing:
    raise SystemExit("TTY-HARNESS:FAIL:unsent-input:" + ",".join(missing))
if returncode != 0:
    raise SystemExit(f"TTY-HARNESS:FAIL:qemu-exit:{returncode}")
if b"TTYTEST:SUMMARY:PASS:" not in data or b"TTYTEST:FAIL:" in data:
    raise SystemExit("TTY-HARNESS:FAIL:guest-summary")
vi_start = data.find(b"@@TTY VI auto-start@@")
vi_end = data.find(b"TTYTEST:PASS:busybox-vi-auto", vi_start + 1)
if (
    vi_start < 0
    or vi_end < 0
    or vi_seed not in data[vi_start:vi_end]
    or b"\x1b[29;1H" not in data[vi_start:vi_end]
):
    raise SystemExit("TTY-HARNESS:FAIL:busybox-vi-winsize")

binary_start = data.find(b"@@TTY OUTPUT binary-begin@@")
binary_end = data.find(b"@@TTY OUTPUT binary-end@@", binary_start + 1)
if binary_start < 0 or binary_end < 0 or b"\x00\xffA" not in data[binary_start:binary_end]:
    raise SystemExit("TTY-HARNESS:FAIL:binary-write-bytes")
onlcr_start = data.find(b"@@TTY OUTPUT onlcr-begin@@")
onlcr_end = data.find(b"@@TTY OUTPUT onlcr-end@@", onlcr_start + 1)
if onlcr_start < 0 or onlcr_end < 0 or b"X\r\nY" not in data[onlcr_start:onlcr_end]:
    raise SystemExit("TTY-HARNESS:FAIL:onlcr-bytes")
before = data.find(b"@@TTY DRAIN before@@")
payload = data.find(b"DRAIN-PAYLOAD", before + 1)
after = data.find(b"@@TTY DRAIN after@@", payload + 1)
if not (0 <= before < payload < after):
    raise SystemExit("TTY-HARNESS:FAIL:tcsetsw-order")

message = b"TTY-HARNESS:PASS:auto-byte-checks\n"
with open(log_path, "ab", buffering=0) as log:
    log.write(message)
sys.stdout.buffer.write(message)
PY
else
    progress "qemu-manual-vi"
    just xtask qemu --platform "$platform" --image build/anemone.elf 2>&1 | tee -a "$log_file"
    grep -aq 'TTYTEST:SUMMARY:PASS:' "$log_file" || fail "manual vi summary did not pass"
    if grep -aq 'TTYTEST:FAIL:' "$log_file"; then
        fail "manual vi log contains a failed case"
    fi
fi

progress "PASS:$mode:$log_file" | tee -a "$log_file"
