[doc("the command invoked when just is run without arguments")]
default:
    @just --list

[doc("run an xtask command, e.g. `just xtask help`")]
xtask *args:
    @cd scripts/xtask && cargo run -q -- {{ args }}

[doc("run a repository-owned test suite: `xtask`, `symtab`, `net-host`, `virtio-drivers`, or `lwext4`")]
test suite:
    @case {{ quote(suite) }} in \
        xtask) just test-xtask ;; \
        symtab) just test-symtab ;; \
        net-host) just test-net-host ;; \
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
    @cargo test -p anemone-smoltcp-stack --no-default-features --no-run
    @cargo check -p anemone-smoltcp-stack --no-default-features

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

[doc("format Rust sources in the explicit `all`, `kernel`, or app scope")]
fmt scope *args:
    @just xtask fmt {{ scope }} {{ args }}

[doc("manage configurations. type `just conf -h` for more details.")]
conf *args:
    @just xtask conf {{ args }}

[doc("app related commands. type `just app -h` for more details.")]
app *args:
    @just xtask app {{ args }}

[doc("manage curated external source references")]
xref *args:
    @just xtask xref {{ args }}

[doc("rootfs management. type `just rootfs -h` for more details.")]
rootfs *args:
    @just xtask rootfs {{ args }}

[doc("generate the kconfig file from .defconfig")]
defconfig:
    @just log "DEFCONFIG" "Copying .defconfig to kconfig"
    @cp conf/.defconfig ./kconfig

[private]
log topic msg:
    @printf "  \\033[1;96m%10s\\033[0m \\033[1;m%s\\033[0m\\n" "{{ topic }}" "{{ msg }}"
