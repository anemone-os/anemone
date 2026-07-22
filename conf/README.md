# Build configuration layout

- `conf/system-targets/<slug>.toml` owns root and initial-program selection and references one
  Platform.
- `conf/platforms/<slug>.toml` owns machine, DT, QEMU and kernel-output facts. It does not own root
  selection. A Platform with `[dtb]` names a committed DTS and declares whether that source is a
  provider-derived firmware conformance baseline or the normative source for an embedded DTB.
- Normal kernel build compiles the selected Platform DTS to
  `build/generated/device-tree/platform.dtb` with `dtc`. It does not start QEMU or consume runtime
  disk/network inputs to obtain a device tree.
- `conf/.defconfig` and local `kconfig` still carry a temporary `[build].target` selection together
  with kernel Cargo profile and presentation. This legacy bridge exists only until the Stage 2 CLI
  cutover; new selection sources must not reuse it.

TODO:
- explain the necessity of certain settings in target spec json files;
- refine the remaining platform schema and add a KernelConfig schema.
