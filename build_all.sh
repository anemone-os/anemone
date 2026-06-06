#!/usr/bin/env bash
export RUSTUP_UPDATE_ROOT=https://mirrors.aliyun.com/rustup/rustup
export RUSTUP_DIST_SERVER=https://mirrors.aliyun.com/rustup

set -euo pipefail

cargo install just
cp conf/.benchconf kconfig
just conf switch rv
just build
cp build/anemone.elf kernel-rv
just conf switch la
just build
cp build/anemone.elf kernel-la
