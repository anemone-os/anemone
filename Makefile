RUSTUP_DIST_SERVER ?= https://rsproxy.cn
RUSTUP_UPDATE_ROOT ?= https://rsproxy.cn/rustup

export RUSTUP_DIST_SERVER
export RUSTUP_UPDATE_ROOT

XTASK := cd scripts/xtask && cargo run --quiet --locked --

# Both architecture builds write shared generated kernel inputs.
.NOTPARALLEL:
.PHONY: all kernel-rv kernel-la

all: kernel-rv kernel-la

kernel-rv:
	rm -f $@
	$(XTASK) build --preset competition-final-rv64-release
	cp build/anemone.elf $@

kernel-la:
	rm -f $@
	$(XTASK) build --preset competition-final-la64-release
	cp build/anemone.elf $@
