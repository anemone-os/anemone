# Loongson 2K1000 RootFS

This directory contains the configuration and local inputs used to build the
Loongson 2K1000 root filesystem.

The root filesystem is materialized from the folder tree under `base/` plus the
files declared in `rootfs.toml`. The generated image is written to:

```text
build/rootfs/2k1000/rootfs.img
```

## Disk Layout and Boot Flow

The generated image is the Anemone bootstrap root filesystem. It is written to
the third partition of the boot disk, which Anemone exposes as `sda3`. The
runtime mount sequence is:

1. Anemone mounts `sda3` as its initial root `/` and starts `/sbin/board-init`.
2. `board-init` mounts the Linux ext4 image from `sda2` at `/linux`.
3. `board-init` mounts the same `sda3` filesystem again at `/linux/home`, so the
   bootstrap files remain available as Linux `/home`.
4. After preparing the Linux tree, `board-init` calls `chroot("/linux")` and
   executes `/usr/sbin/init` inside the Linux image.

The raw Anemone kernel is stored separately on the first partition and is not
duplicated inside the bootstrap root filesystem.

## Required Inputs

The folder under `base/` must provide a static LoongArch BusyBox at
`base/bin/busybox` so `board-init` can prepare the Linux root before entering
the chroot.

The Linux image installed on `sda2` must be a strict-alignment LoongArch
userspace. Its executable code, including init, libc, BusyBox, compilers, and
other tools, must be built with `-mstrict-align` or an equivalent
strict-alignment toolchain configuration. Do not rely on Anemone's software
unaligned-access emulation as the normal execution path for this image.

The raw kernel image is a fixed-path handoff from the Platform build. From the
repository root, use this order with the same 2K1000 selection:

```text
source .envrc
just build --preset 2k1000-la64-release
just rootfs mkfs -c conf/rootfs/2k1000/rootfs.toml --sudo
```

`etc/build-la.sh` installs `build/anemoneImage-la64-raw` on the first partition
and writes `build/rootfs/2k1000/rootfs.img` to the third partition (`sda3`). It
does not modify the strict-alignment Linux root filesystem on `sda2`.
