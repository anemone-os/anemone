# debugging on qemu for qemu-virt-rv64
set shell := ["bash", "-uc"]
set working-directory := "../"

default:
    @just -f scripts/qemu-virt-rv64-dbg.just -d scripts --list

run:
    qemu-system-riscv64 \
        -machine virt \
        -nographic \
        -m 1G \
        -smp 4 \
        -bios default \
        -kernel build/anemone.elf \
        -serial mon:stdio

qemu-server:
    qemu-system-riscv64 \
        -machine virt \
        -cpu rv64 \
        -nographic \
        -m 1G \
        -smp 4 \
        -bios default \
        -kernel build/anemone.elf \
        -drive \
            file=sdcard-rv.img,format=raw,if=none,id=x0 \
        -device \
            virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
        -no-reboot \
        -drive \
            file=build/rootfs/minimal-rv/rootfs.img,format=raw,if=none,id=x1 \
        -device \
            virtio-blk-device,drive=x1,bus=virtio-mmio-bus.1 \
        -serial mon:stdio \
        -S -s

gdb-client:
    RUST_GDB=riscv64-unknown-elf-gdb rust-gdb \
        -ex "target remote :1234" \
        -ex "file build/anemone.elf" \
        -ex "set architecture riscv:rv64"

lldb-client:
    lldb \
        -o "file build/anemone.elf" \
        -o "gdb-remote 1234"
