#!/usr/bin/env bash
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
export RUSTUP_DIST_SERVER=https://rsproxy.cn

set -euo pipefail

apt install libguestfs-tools
cargo install just
cp conf/.benchconf kconfig

# Make disk images
rm -rf rv-base la-base
mkdir -p rv-base/dev la-base/dev
mkdir -p rv-base/mnt la-base/mnt


dd if=/dev/zero of=sdcard-rv.img bs=1M count=1 &
dd if=/dev/zero of=sdcard-la.img bs=1M count=1 &
just rootfs mkfs --config conf/rootfs/test-rv &
just rootfs mkfs --config conf/rootfs/test-la &
wait 
cp build/rootfs/test-rv/rootfs.img disk.img
cp build/rootfs/test-la/rootfs.img disk-la.img


# RV
just conf switch pre-test-rv64
just build
cp build/anemone.elf kernel-rv
rm -rf sdcard-rv.img

# LA
just conf switch pre-test-la64
just build
cp build/anemone.elf kernel-la

rm -rf sdcard-la.img