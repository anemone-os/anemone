# VisionFive 2 RootFS

This directory contains the configuration and local inputs used to build the
VisionFive 2 root filesystem.

The root filesystem is materialized from the folder tree under `base/` plus the
files declared in `rootfs.toml`. The generated image is written to:

```text
build/rootfs/visionfive2/rootfs.img
```

## Disk Layout and Boot Flow

The generated image is the Anemone bootstrap root filesystem. It is written to
the third partition of the boot disk, which Anemone exposes as `mmcblk0p3`. The
runtime mount sequence is:

1. Anemone mounts `mmcblk0p3` as its initial root `/` and starts
   `/sbin/board-init`.
2. `board-init` mounts the Linux ext4 image from `mmcblk0p2` at `/linux`.
3. `board-init` mounts the same `mmcblk0p3` filesystem again at `/linux/home`,
   so the bootstrap files remain available as Linux `/home`.
4. After preparing the Linux tree, `board-init` calls `chroot("/linux")` and
   executes `/usr/sbin/init` inside the Linux image.

The bootable Anemone kernel is installed on the first partition. The rootfs
manifest also carries the same build output at `/boot/anemoneImage` inside the
bootstrap image as a fixed-path handoff.

## Required Inputs

The folder under `base/` must provide a static RISC-V BusyBox at
`base/bin/busybox` so `board-init` can prepare the Linux root before entering
the chroot.

The Linux ext4 image on `mmcblk0p2` must contain the complete riscv64 userspace
used after the chroot. It must provide the LP64D musl interpreter at
`/lib/ld-musl-riscv64.so.1` and the native GNU tools required by the tests,
including GCC, binutils, development headers, libraries, and make.

The kernel image is a fixed-path handoff from the Platform build. From the
repository root, use this order with the same `visionfive2-rv64` selection:

```text
source .envrc
just build --preset visionfive2-rv64-release
just rootfs mkfs -c conf/rootfs/visionfive2/rootfs.toml --sudo
```

Do not skip the build because `build/anemoneImage-rv64` already exists. The
rootfs action does not track which invocation produced that path. If an
untracked compiler, linker, sysroot, `rust-objcopy`, or `mkimage` input changes,
run `just clean` before the build. `just clean` removes the complete `build/`
tree, so never run it between the build and rootfs commands above. If the build
fails, do not run the rootfs command.

`etc/build.sh` installs the Anemone kernel on the first partition and writes
`build/rootfs/visionfive2/rootfs.img` to the third partition (`mmcblk0p3`). It
does not modify the Linux root filesystem on `mmcblk0p2`.
