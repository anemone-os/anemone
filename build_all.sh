export RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup/rustup
export RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static/rustup
bash ./setup_mirror.sh
cargo install just
cp conf/.benchconf kconfig
just conf switch rv
just build
cp build/anemone.elf kernel-rv
just conf switch la
just build
cp build/anemone.elf kernel-la