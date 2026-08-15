[doc("the command invoked when just is run without arguments")]
default:
    @just --list

[doc("run an xtask command, e.g. `just xtask help`")]
xtask *args:
    @cd scripts/xtask && cargo run -q -- {{ args }}

[doc("run a repository-owned test suite: `xtask`, `symtab`, `net-host`, `nemophila-wasm`, `nemophila-module`, `virtio-drivers`, or `lwext4`")]
test suite:
    @case {{ quote(suite) }} in \
        xtask) just test-xtask ;; \
        symtab) just test-symtab ;; \
        net-host) just test-net-host ;; \
        nemophila-wasm) just test-nemophila-wasm ;; \
        nemophila-module) just test-nemophila-module ;; \
        virtio-drivers) just test-virtio-drivers ;; \
        lwext4) just test-lwext4 ;; \
        *) echo "unknown test suite:" {{ quote(suite) }} >&2; exit 2 ;; \
    esac

[private]
test-xtask:
    @cd scripts/xtask && cargo test

[private]
test-symtab:
    @cargo test -p symtab --all-features

[private]
test-net-host:
    @cargo test -p anemone-net-api -p anemone-smoltcp-stack
    @cargo test -p smoltcp --lib --no-default-features --features std,medium-ethernet,medium-ip,proto-ipv4,proto-ipv4-fragmentation,socket-raw,socket-udp,auto-icmp-echo-reply iface::interface::tests::ipv4
    @cargo test -p smoltcp --lib --no-default-features --features std,medium-ip,proto-ipv4,socket-tcp socket::tcp::test::
    @cargo test -p anemone-smoltcp-stack --no-default-features --no-run
    @cargo check -p anemone-smoltcp-stack --no-default-features

[private]
test-nemophila-wasm:
    @cargo check -p nemophila-wasm --no-default-features --features extra-checks
    @cargo test -p nemophila-wasm --features host-test
    @cargo miri test -p nemophila-wasm --features host-test integration::stage1_embedding
    @cargo rustc -p nemophila-wasm-embed-validation --target riscv64gc-unknown-none-elf -- -C panic=abort
    @cargo rustc -p nemophila-wasm-embed-validation --target loongarch64-unknown-none -- -C panic=abort

[private]
test-nemophila-module:
    @just module build clone-observer

[private]
test-virtio-drivers:
    @cargo test -p virtio-drivers --no-default-features
    @cargo test -p virtio-drivers --all-features

[private]
test-lwext4:
    @LWEXT4_CC="${LWEXT4_CC:-cc}" \
        LWEXT4_CXX="${LWEXT4_CXX:-c++}" \
        LWEXT4_AR="${LWEXT4_AR:-ar}" \
        LWEXT4_SYSROOT="${LWEXT4_SYSROOT:-/}" \
        cargo test -p lwext4_rust --lib

[doc("clean the build artifacts of Anemone kernel")]
clean:
    @just xtask clean

[doc("build Anemone kernel")]
build *args:
    @just xtask build {{ args }}

[doc("run Anemone with the selected QEMU Platform")]
qemu *args:
    @just xtask qemu {{ args }}

[doc("format Rust sources in an explicit `all`, `kernel`, `modules`, app, or module scope")]
fmt scope *args:
    @just xtask fmt {{ scope }} {{ args }}

[doc("manage configurations. type `just conf -h` for more details.")]
conf *args:
    @just xtask conf {{ args }}

[doc("app related commands. type `just app -h` for more details.")]
app *args:
    @just xtask app {{ args }}

[doc("build and export a Nemophila module by identity")]
module *args:
    @just xtask module {{ args }}

[doc("manage curated external source references")]
xref *args:
    @just xtask xref {{ args }}

[doc("rootfs management. type `just rootfs -h` for more details.")]
rootfs *args:
    @just xtask rootfs {{ args }}

[doc("generate kconfig from the tracked default KernelConfig")]
defconfig:
    @just log "DEFCONFIG" "Copying the default KernelConfig to kconfig"
    @cp conf/kconfs/default.toml ./kconfig

[private]
log topic msg:
    @printf "  \\033[1;96m%10s\\033[0m \\033[1;m%s\\033[0m\\n" "{{ topic }}" "{{ msg }}"
