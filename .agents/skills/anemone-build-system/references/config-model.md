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

Root `kconfig` selects kernel build policy and a platform. `conf/.defconfig` owns tracked defaults. Before changing either, inspect how the build task parses the selected file, which values may fall back, and which generated definitions it writes.

### Platform And Architecture

`conf/platforms/` owns a platform's architecture-facing and launch-facing configuration. `conf/arch/` owns architecture templates and target specifications. The build, configuration, QEMU, and DTB tasks may consume different parts of this layer; inspect every consumer affected by a change.

### App Manifest

`anemone-apps/<app>/app.toml` owns the app build driver and artifact export contract. The app task locates the manifest, runs its driver for a chosen architecture, and exports declared artifacts. Keep locator name, manifest identity, driver profile, and artifact path coherent.

### Rootfs Manifest

`conf/rootfs/` owns filesystem composition: architecture, base tree, init, apps, directories, and host files. Rootfs construction consumes app manifests and produces an image used by some platform/QEMU flows. Keep composition inputs and the consuming platform aligned.

## Cross-Layer Invariants

Before executing or accepting a configuration change, verify:

- kconfig platform selection resolves to the intended platform;
- platform architecture agrees with target, linker, DTB, firmware, and QEMU choices;
- app architecture, driver output, and declared export agree;
- rootfs architecture and installed apps agree with the intended kernel;
- rootfs output and QEMU device/image wiring agree when the platform uses that image;
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
