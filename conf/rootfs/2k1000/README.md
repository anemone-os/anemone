# Loongson 2K1000 RootFS

This directory contains the Anemone bootstrap root filesystem written to:

```text
build/rootfs/2k1000/rootfs.img
```

## Disk Layout and Boot Flow

The bootstrap image is written to the third partition, exposed by Anemone as
`sda3`. The strict Alpine system image is a separate ext4 image written only to
`sda2`. The runtime sequence is:

1. Anemone mounts `sda3` as `/` and starts `/sbin/board-init`.
2. `board-init` mounts `sda2` at `/linux` and `sda3` at `/linux/home`.
3. `board-init` chroots to `/linux` and runs `/home/init_sys.sh` from `sda3`.
4. The script mounts devfs, proc, `/run`, and `/tmp` inside the Alpine root.
5. `board-init` executes Alpine `/sbin/init`; OpenRC starts firstboot, Nginx,
   and OpenSSH.

The raw Anemone kernel is stored separately on the first partition and is not
duplicated inside the bootstrap root filesystem.

## Required Inputs

Build `build/alpine-loongarch64-strict-system.img` for `sda2`. Its executable code,
including init, libc, BusyBox, compilers, Nginx, and OpenSSH, is produced by the
strict local APK pipeline under `containers/alpine-loongarch64-strict`.
Before writing it, the provisioning script injects this device's TLS/SSH keys,
machine id, and random seed using host cryptographic randomness. The board
does not generate private keys from Anemone's temporary random implementation.
The bootstrap tree under `base/` provides the static BusyBox and pre-init
script used before Alpine init starts.

The raw kernel image is a fixed-path handoff from the Platform build. From the
repository root, use this order with the same 2K1000 selection:

```text
source .envrc
docker buildx build \
  --platform linux/amd64 \
  --file containers/alpine-loongarch64-strict/Containerfile \
  --target system-image-artifact \
  --output type=local,dest=build \
  containers/alpine-loongarch64-strict
just build --preset 2k1000-la64-release
just rootfs mkfs -c conf/rootfs/2k1000/rootfs.toml --sudo
```

`etc/build-la.sh` installs `build/anemoneImage-la64-raw` on the first partition
and writes the strict Alpine image to `sda2` and the board-init bootstrap image
to `sda3`. The Alpine system image intentionally has no `/.anemone/init`; it is
not a replacement for the `sda3` bootstrap image. The script provisions the
device identity in the Alpine image immediately before unmounting and writing
the disk.
