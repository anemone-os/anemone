[doc("the command invoked when just is run without arguments")]
default:
    @just --list

[doc("run an xtask command, e.g. `just xtask help`")]
xtask *args:
    @cd scripts/xtask && cargo run -q -- {{ args }}

[private]
xtask-test:
    @cd scripts/xtask && cargo test

[doc("clean the build artifacts of Anemone kernel")]
clean:
    @just xtask clean

[doc("build Anemone kernel")]
build *args:
    @just xtask build {{ args }}

[doc("run Anemone with the selected QEMU Platform")]
qemu *args:
    @just xtask qemu {{ args }}

[doc("format Rust sources. optionally pass `kernel` or an app name")]
fmt *args:
    @just xtask fmt {{ args }}

[doc("manage configurations. type `just conf -h` for more details.")]
conf *args:
    @just xtask conf {{ args }}

[doc("manage the developer-local interactive selection")]
selection *args:
    @just xtask selection {{ args }}

[doc("app related commands. type `just app -h` for more details.")]
app *args:
    @just xtask app {{ args }}

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
