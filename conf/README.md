# Build configuration layout

- `conf/system-targets/<slug>.toml` owns root and initial-program selection and references one
  Platform.
- `conf/platforms/<slug>.toml` owns machine, DT, QEMU and kernel-output facts. It does not own root
  selection. A Platform with `[dtb]` declares runtime delivery, authority and provider. Physical
  Platforms also name a committed DTS source; QEMU Platforms do not keep a provider-derived mirror.
- Normal kernel build removes stale DTB output for firmware delivery. For embedded delivery it
  either compiles a physical normative DTS with `dtc`, or asks the selected QEMU provider to dump a
  build-local DTB using only machine, CPU, SMP, memory and optional BIOS. It never consumes ordinary
  QEMU args, runtime disk/network inputs or bind values to obtain a device tree.
- `conf/.defconfig` and local `kconfig` contain only kernel features, policy and capacity. System
  selection, kernel Cargo profile and action-local presentation do not belong to KernelConfig.
- `conf/build-presets/<slug>.toml` names a closed target, workspace-relative KernelConfig and
  kernel-only Cargo profile tuple. Presets contain no action presentation defaults.
- Build and ordinary QEMU require either `--preset` or the complete `--target` + `--kernel-config`
  + `--profile` tuple. They have no local or repository-default selection. QEMU host paths are
  supplied only through the selected
  Platform's ordered `[[qemu.bind]]` declarations.
- Every QEMU Platform names its CPU model explicitly. `bios` remains optional: omission means xtask
  emits no `-bios` option.
- Every rootfs manifest names `fs.type` explicitly. Folder roots always use `virt-make-fs`
  automatic sizing; capacity is not configurable through the manifest.
- QEMU has no DT refresh/check command or source write-back path. Firmware delivery consumes the
  runtime FDT; embedded QEMU delivery is materialized only by normal build. Physical capture
  provenance, allowed runtime differences and validation responsibility remain human-reviewed
  Platform maintenance facts, not machine-maintained configuration.

TODO:
- explain the necessity of certain settings in target spec json files;
- refine the remaining platform schema and add a KernelConfig schema.
