[doc("the command invoked when just is run without arguments")]
default:
    @just --list

[doc("clean the build artifacts of xtask itself")]
xtask-clean:
    @cd scripts/xtask && cargo clean

[doc("run an xtask command, e.g. `just xtask help`")]
xtask *args:
    @cd scripts/xtask && cargo run -q -- {{ args }}

[doc("clean the build artifacts of Anemone kernel")]
clean:
    @just xtask clean

[doc("clean the build artifacts of Anemone kernel, including the configuration files")]
mrproper:
    @just xtask mrproper
    @rm -f disk.img

[doc("build Anemone kernel")]
build:
    @just xtask build

[doc("manage configurations. type `just conf -h` for more details.")]
conf *args:
    @just xtask conf {{ args }}

[doc("generate the kconfig file from .defconfig")]
defconfig:
    @just log "DEFCONFIG" "Copying .defconfig to kconfig"
    @cp conf/.defconfig ./kconfig

[doc("generate an empty disk image")]
gendisk size:
    @just log "GENDISK" "Generating disk image"
    @rm -f disk.img
    @fallocate -l {{ size }} disk.img

[private]
log topic msg:
    @printf "  \\033[1;96m%10s\\033[0m \\033[1;m%s\\033[0m\\n" "{{ topic }}" "{{ msg }}"
