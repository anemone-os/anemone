# VisionFive 2 RootFS

This directory contains the configuration and local inputs used to build the
VisionFive 2 root filesystem.

The root filesystem starts from `rootfs-alpine.img`, then applies the files in
`base/` and the files declared in `rootfs.toml`. The generated image is written
to:

```text
build/rootfs/visionfive2/rootfs.img
```

## Required Inputs

`rootfs-alpine.img` must be a pre-sized raw ext4 image containing a complete
Alpine riscv64 userspace. It must provide the LP64D musl interpreter at
`/lib/ld-musl-riscv64.so.1` and the native GNU tools required by the tests,
including GCC, binutils, development headers, libraries, and make. The rootfs
task copies this image before modifying it and never resizes the source image.
