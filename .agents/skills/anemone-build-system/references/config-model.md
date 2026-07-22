# Build Configuration Model

Use this reference for stable ownership and consistency rules. Derive current fields, defaults, paths, and supported variants from live source.

## Authority

Resolve disagreements in this order:

1. config types under `scripts/xtask/src/config/`;
2. owning tasks under `scripts/xtask/src/tasks/` and the Justfile;
3. active configuration selected by the command;
4. examples, schemas, comments, and general prose.

Do not copy a current configuration snapshot into the skill. Point to its owner and require a fresh read.

## Layers

### Kernel Configuration

Root `kconfig` and `conf/.defconfig` own kernel feature, policy, and capacity values. Until the selection CLI cutover, root `kconfig` also carries a legacy SystemTarget/profile/presentation selection bridge; the resolver must keep that bridge outside the owned KernelConfig value. Before changing either file, inspect which values may fall back and which generated definitions the build writes.

### System Target

`conf/system-targets/` owns the selected Platform reference, root mount/source, and initial-program source. A SystemTarget does not own machine constants, kernel parameters, kernel Cargo profile, QEMU invocation values, or Platform kernel-output formats.

### Platform And Architecture

`conf/platforms/` owns a platform's architecture-facing and launch-facing configuration plus boot-ABI-required kernel output formats. `conf/arch/` owns architecture templates and target specifications. The build, configuration, QEMU, and DTB tasks may consume different parts of this layer; inspect every consumer affected by a change.

### App Manifest

`anemone-apps/<app>/app.toml` owns the app build driver and artifact export contract. The app task locates the manifest, runs its driver for a chosen architecture, and exports declared artifacts. Keep locator name, manifest identity, driver profile, and artifact path coherent.

### Rootfs Manifest

`conf/rootfs/` owns filesystem composition: architecture, base tree, init, apps, directories, and host files. Rootfs construction consumes app manifests and may consume fixed-path outputs from a prior repository action. The recipe or adjacent documentation owns that command order; the rootfs task does not infer invocation history or freshness from path existence.

## Cross-Layer Invariants

Before executing or accepting a configuration change, verify:

- the selected SystemTarget resolves to the intended Platform;
- the legacy kconfig bridge, while present, resolves the intended SystemTarget and does not leak into the owned KernelConfig value;
- platform architecture agrees with target, linker, DTB, firmware, and QEMU choices;
- the build produces every kernel output required by the selected Platform;
- app architecture, driver output, and declared export agree;
- rootfs architecture and installed apps agree with the intended kernel;
- rootfs output and QEMU device/image wiring agree when the platform uses that image;
- fixed-path consumers run after their documented producer and stop when it fails;
- cleanup and wrapper behavior does not invalidate another layer's required input;
- validation observes outputs from the current invocation, not stale conditional artifacts.

These are system invariants even when xtask does not enforce all of them directly.

## DTB Changes

Treat DTB generation as a cross-cutting build concern. Before modifying it, read the live platform model, build task, QEMU command construction, architecture paths, and active platform files. Determine which inputs affect build-time generation versus runtime launch, and validate both paths when their contract changes.

Do not describe a proposed DTB workflow as an existing capability. Keep future design in the appropriate plan or RFC until code and commands implement it.

## Staleness Check

When updating this skill later:

1. remove facts that merely describe current filenames, platform values, or temporary limitations;
2. retain owner boundaries, decision rules, and safety checks;
3. prefer instructions to inspect live help/code over duplicated option tables;
4. add a concrete detail only when using the skill safely would otherwise be impractical.
