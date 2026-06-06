#!/usr/bin/env bash
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
export RUSTUP_DIST_SERVER=https://rsproxy.cn

set -euo pipefail

cargo install just
cp conf/.benchconf kconfig
just conf switch rv
just build
cp build/anemone.elf kernel-rv
just conf switch la
just build
cp build/anemone.elf kernel-la
