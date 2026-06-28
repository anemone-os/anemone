# Build Playbook

## Use This Command Matrix

| Task                                | Command                                                               | Primary outputs                                                 | Notes                                                                                                                     |
| ----------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Initialize config                   | `just defconfig`                                                      | `kconfig`                                                       | Copy `conf/.defconfig` to the repository root.                                                                            |
| List platforms                      | `just conf list`                                                      | none                                                            | Read `conf/platforms/*.toml`.                                                                                             |
| Switch platform                     | `just conf switch qemu-virt-rv64`                                     | updated `kconfig`                                               | Update only `[build].platform` inside `kconfig`.                                                                          |
| Build kernel                        | `just build`                                                          | `build/anemone.elf`, `build/anemone.disasm`, `build/kernel.map` | Regenerate `anemone-kernel/src/kconfig_defs.rs`, `anemone-kernel/src/platform_defs.rs`, and `build/generated/kernel.lds`. |
| Build one app                       | `just xtask app build init --arch riscv64`                            | `build/apps/init/`                                              | Let xtask choose the target spec and copy artifacts out of the app-local `target/` tree.                                  |
| Build one app with extra cargo args | `just xtask app build init --arch riscv64 -- --release`               | `build/apps/init/`                                              | Pass extra driver args after `--`. Keep using xtask as the outer entrypoint.                                              |
| Build rootfs                        | `just xtask rootfs mkfs -c conf/rootfs/minimal.toml`                  | `build/rootfs/minimal/root/`, `build/rootfs/minimal/rootfs.img` | Stage files, build listed apps, and create an ext4 image.                                                                 |
| Run QEMU                            | `just xtask qemu --platform qemu-virt-rv64 --image build/anemone.elf` | runtime only                                                    | Ensure every image path referenced by the selected platform config already exists.                                        |
| Clean build outputs                 | `just clean`                                                          | none                                                            | Remove `build/` and run the repo-approved cargo cleanup path.                                                             |
| Reset generated config              | `just mrproper`                                                       | none                                                            | Remove `build/`, cargo outputs, generated defs, `kconfig`, and `disk.img`.                                                |

## Inspect These Files First

- `Justfile`
- `scripts/xtask/src/tasks/build/mod.rs`
- `scripts/xtask/src/tasks/app/build.rs`
- `scripts/xtask/src/tasks/app/driver/cargo.rs`
- `scripts/xtask/src/tasks/rootfs/mkfs.rs`
- `scripts/xtask/src/tasks/qemu.rs`
- `scripts/xtask/src/tasks/conf.rs`
- `conf/.defconfig`
- `conf/platforms/*.toml`
- `conf/rootfs/*.toml`
- `anemone-apps/*/app.toml`

## Apply These Repo-Specific Caveats

1. Treat `target/` as implementation detail. Read `build/` for user-facing outputs first.
2. Remember that `just build` does not create a rootfs image. Run `just xtask rootfs mkfs ...` separately when QEMU or disk boot depends on one.
3. Remember that app manifests define exported artifacts. Adjust `anemone-apps/<app>/app.toml` instead of copying files manually from `target/`.
4. Remember that app artifact paths expand `${ARCH}` and `${TARGET_TRIPLE}`. Preserve those placeholders unless you are intentionally changing the driver contract.
5. Remember that rootfs generation is currently ext4-only. Change `scripts/xtask/src/config/rootfs.rs` and `scripts/xtask/src/config/build.rs` before documenting another filesystem.
6. Remember that `conf/platforms/qemu-virt-rv64.toml` currently hard-codes `build/rootfs/minimal/rootfs.img` in QEMU args. Keep rootfs naming and QEMU wiring aligned.