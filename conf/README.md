# Build configuration layout

- `conf/system-targets/<slug>.toml` owns root and initial-program selection and references one
  Platform.
- `conf/platforms/<slug>.toml` owns machine, DT, QEMU and kernel-output facts. It does not own root
  selection. A Platform with `[dtb]` names a committed DTS and independently declares runtime
  delivery, source authority and provider. Embedded delivery does not imply normative authority.
- Normal kernel build compiles the selected Platform DTS to
  `build/generated/device-tree/platform.dtb` with `dtc`. It does not start QEMU or consume runtime
  disk/network inputs to obtain a device tree.
- `conf/.defconfig` and local `kconfig` contain only kernel features, policy and capacity. System
  selection, kernel Cargo profile and action-local presentation do not belong to KernelConfig.
- `conf/build-presets/<slug>.toml` names a closed target, workspace-relative KernelConfig and
  kernel-only Cargo profile tuple. `conf/default-selection.toml` selects one tracked preset, while
  ignored `conf/.selection.toml` is reserved for a developer-local preset selection.
- Build and ordinary QEMU share one selection resolver. Explicit callers use either `--preset` or
  the complete `--target` + `--kernel-config` + `--profile` tuple; interactive calls may use the
  local/default preset reference. QEMU host paths are supplied only through the selected
  Platform's ordered `[[qemu.bind]]` declarations.
- `just qemu dt refresh --platform <qemu-platform> [--check]` maintains only a
  `provider-derived` baseline whose provider is `qemu`. It loads the Platform directly, uses only
  its topology fields, and never consumes ordinary selection or runtime binds. A QEMU-backed
  normative DTS is check-only; physical `provider = "firmware"` baselines fail closed and must carry
  closed provenance, allowed-runtime-difference and runtime-validation-owner metadata.

TODO:
- explain the necessity of certain settings in target spec json files;
- refine the remaining platform schema and add a KernelConfig schema.
