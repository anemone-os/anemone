#!/usr/bin/env bash
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
export RUSTUP_DIST_SERVER=https://rsproxy.cn

set -euo pipefail

apt install libguestfs-tools
cargo install just
cp conf/.benchconf kconfig

# RV
# for build
rm -rf rv-base
mkdir -p rv-base/dev
mkdir -p rv-base/mnt
dd if=/dev/zero of=sdcard-rv.img bs=1M count=1
just conf switch pre-test-rv64
just rootfs mkfs --config conf/rootfs/test-rv
cp build/rootfs/test-rv/rootfs.img disk.img
just build
cp build/anemone.elf kernel-rv

# LA
# rm -rf la-base
# mkdir -p la-base/dev
# mkdir -p la-base/mnt
# just conf switch pre-test-la64
# just rootfs mkfs --config conf/rootfs/test-la
# cp build/rootfs/test-la/rootfs.img disk-la.img
# just build
# cp build/anemone.elf kernel-la