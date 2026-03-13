# debugging on qemu for qemu-virt-rv64
set shell := ["bash", "-uc"]
set working-directory := "../"

default:
    @just -f scripts/qemu-virt-rv64-dbg.just -d scripts --list

run:
    qemu-system-riscv64 \
        -machine virt \
        -nographic \
        -m 256M \
        -smp 4 \
        -bios default \
        -kernel build/anemone.elf \
        -serial mon:stdio

qemu-server:
    qemu-system-riscv64 \
        -machine virt \
        -nographic \
        -m 256M \
        -smp 4 \
        -bios default \
        -kernel build/anemone.elf \
        -serial mon:stdio \
        -S -s

gdb-client:
    riscv64-unknown-elf-gdb \
        -ex "target remote :1234" \
        -ex "file build/anemone.elf" \
        -ex "set architecture riscv:rv64"