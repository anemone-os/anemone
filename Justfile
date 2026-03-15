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

[doc("build Anemone kernel")]
build:
    @just xtask build

[doc("list all available build configurations and their abbreviations")]
list:
    @just xtask list

[doc("switch to a different build configuration")]
switch *args:
    @just xtask switch {{ args }}

[doc("generate the kconfig file from .defconfig")]
defconfig:
    @just log "DEFCONFIG" "Copying .defconfig to kconfig"
    @cp conf/.defconfig ./kconfig

[private]
log topic msg:
    @printf "  \\033[1;96m%10s\\033[0m \\033[1;m%s\\033[0m\\n" "{{ topic }}" "{{ msg }}"
