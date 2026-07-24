# Loongson 2K1000 RootFS

This directory contains the configuration and local inputs used to build the
Loongson 2K1000 root filesystem.

The root filesystem is materialized from the folder tree under `base/` plus the
files declared in `rootfs.toml`. The generated image is written to:

```text
build/rootfs/2k1000/rootfs.img
```

## Required Inputs

`base/busybox` must be a valid LoongArch BusyBox binary. The remaining ignored
content under `base/` supplies the strict-align musl/GCC userspace tree.

The raw kernel image is a fixed-path handoff from the Platform build. From the
repository root, use this order with the same 2K1000 selection:

```text
source .envrc
just build --preset 2k1000-la64-release
just rootfs mkfs -c conf/rootfs/2k1000/rootfs.toml --sudo
```

Do not run the rootfs action after a failed or stale kernel build. The rootfs
task consumes `build/anemoneImage-la64-raw` but does not track which invocation
produced it.
