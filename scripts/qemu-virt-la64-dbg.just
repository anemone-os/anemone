# debugging on qemu for qemu-virt-la64
set shell := ["bash", "-uc"]
set working-directory := "../"

default:
    @just -f scripts/qemu-virt-la64-dbg.just -d scripts --list

run:
    qemu-system-loongarch64 \
        -machine virt \
        -cpu la464 \
        -nographic \
        -m 1G \
        -smp 2 \
        -kernel build/anemone.elf \
        -serial mon:stdio

qemu-server:
    qemu-system-loongarch64 \
        -machine virt \
        -nographic \
        -m 4G \
        -smp 2 \
        -kernel build/anemone.elf \
        -serial mon:stdio \
        -S -s

gdb-client:
    gdb build/anemone.elf \
        -ex "target remote :1234"

lldb-client:
    lldb \
        -o "file build/anemone.elf" \
        -o "gdb-remote 1234"