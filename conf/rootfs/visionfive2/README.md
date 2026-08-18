# VisionFive 2 RootFS

This directory contains the Anemone bootstrap root filesystem written to:

```text
build/rootfs/visionfive2/rootfs.img
```

## Disk Layout and Boot Flow

The bootstrap image is written to the third partition, exposed by Anemone as
`mmcblk0p3`. The Debian system image is a separate ext4 image written only to
`mmcblk0p2`. The runtime sequence is:

1. Anemone mounts `mmcblk0p3` as `/` and starts `/sbin/board-init`.
2. `board-init` mounts `mmcblk0p2` at `/linux` and `mmcblk0p3` at
   `/linux/home`.
3. `board-init` chroots to `/linux` and runs `/home/init_sys.sh` from
   `mmcblk0p3`.
4. The script installs the Anemone overlay and mounts devfs, devpts, proc,
   sysfs, `/run`, and `/tmp` inside the Debian root.
5. `board-init` executes Debian `/sbin/init`; OpenRC enters its default
   runlevel and starts an agetty-owned development shell on `/dev/console`.

The bootable Anemone kernel is installed on the first partition. The rootfs
manifest also carries the same build output at `/boot/anemoneImage` inside the
bootstrap image as a fixed-path handoff.

## Required Inputs

Write `etc/sdcard-rv-pub.img` to `mmcblk0p2`. It contains the complete Debian
riscv64 userspace, including OpenRC as `/sbin/init`. The bootstrap tree under
`base/` provides the static BusyBox and pre-init script used before OpenRC
starts.

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

`etc/build.sh` installs the Anemone kernel on the first partition and updates
only the board-init bootstrap image on `mmcblk0p3`. It never rebuilds or writes
the stable Debian system image on `mmcblk0p2`.
