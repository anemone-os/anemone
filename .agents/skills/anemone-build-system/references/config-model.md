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

Root `kconfig` and `conf/.defconfig` own kernel feature, policy, and capacity values only. Build selection, kernel Cargo profile, action presentation, and QEMU host paths are rejected from KernelConfig. Before changing either file, inspect which parameter values may fall back and which generated definitions the build writes.

### Build Selection

`conf/build-presets/` names a SystemTarget, workspace-relative KernelConfig, and kernel-only Cargo
profile. Tracked `conf/default-selection.toml` and ignored `conf/.selection.toml` each contain only a
preset reference. Build and ordinary QEMU share one resolver; automation uses an explicit preset or
complete low-level tuple, while only interactive calls may use local/default selection.

### System Target

`conf/system-targets/` owns the selected Platform reference, root mount/source, and initial-program source. A SystemTarget does not own machine constants, kernel parameters, kernel Cargo profile, QEMU invocation values, or Platform kernel-output formats.

### Platform And Architecture

`conf/platforms/` owns a platform's architecture-facing and launch-facing configuration plus boot-ABI-required kernel output formats. QEMU sections own fixed argv and ordered bind templates, but never the host executable or invocation path values. `conf/arch/` owns architecture templates and target specifications. The build, configuration, QEMU, and DTB tasks may consume different parts of this layer; inspect every consumer affected by a change.

### App Manifest

`anemone-apps/<app>/app.toml` owns the app build driver and artifact export contract. The app task locates the manifest, runs its driver for a chosen architecture, and exports declared artifacts. Keep locator name, manifest identity, driver profile, and artifact path coherent.

### Rootfs Manifest

`conf/rootfs/` owns filesystem composition: architecture, base tree, init, apps, directories, and host files. Rootfs construction consumes app manifests and may consume fixed-path outputs from a prior repository action. The recipe or adjacent documentation owns that command order; the rootfs task does not infer invocation history or freshness from path existence.

## Cross-Layer Invariants

Before executing or accepting a configuration change, verify:

- the selected SystemTarget resolves to the intended Platform;
- the selected preset or complete low-level tuple resolves the intended SystemTarget and does not merge with interactive local state;
- platform architecture agrees with target, linker, DTB, firmware, and QEMU choices;
- the build produces every kernel output required by the selected Platform;
- app architecture, driver output, and declared export agree;
- rootfs architecture and installed apps agree with the intended kernel;
- every QEMU bind value matches a selected Platform declaration and the intended wrapper mapping;
- fixed-path consumers run after their documented producer and stop when it fails;
- cleanup and wrapper behavior does not invalidate another layer's required input;
- validation observes outputs from the current invocation, not stale conditional artifacts.

These are system invariants even when xtask does not enforce all of them directly.

## DTB Changes

Treat DTB generation as a cross-cutting build concern. Before modifying it, read the live platform model, build task, QEMU command construction, architecture paths, and active platform files. Determine which inputs affect build-time generation versus runtime launch, and validate both paths when their contract changes.

Do not describe a proposed DTB workflow as an existing capability. Keep future design in the appropriate plan or RFC until code and commands implement it.

Normal kernel build compiles the selected Platform's committed DTS into a build-local DTB and does
not launch QEMU or consume runtime disks. A firmware-delivered DTB may use the committed DTS as a
provider conformance baseline; an embedded DTB may consume either a normative source or a QEMU
provider-derived baseline. Delivery and authority are independent. Derive refresh/check capabilities
from live QEMU task code instead of assuming they exist.

A `provider = "qemu"` DT contract grants the QEMU-local maintenance action write authority over a
provider-derived baseline. A `provider = "firmware"` contract records a physical firmware-derived
baseline and grants no QEMU refresh capability. Both remain Platform-owned; neither changes the
runtime FDT acceptance contract. Physical capture provenance, allowed runtime differences, and the
responsibility to revalidate after board or firmware changes are human-reviewed maintenance facts;
keep them adjacent to the Platform baseline without inventing typed fields that no action consumes.
A QEMU-backed normative DTS has a check-only comparison surface; the maintenance action must reject
any attempt to update it.

## Staleness Check

When updating this skill later:

1. remove facts that merely describe current filenames, platform values, or temporary limitations;
2. retain owner boundaries, decision rules, and safety checks;
3. prefer instructions to inspect live help/code over duplicated option tables;
4. add a concrete detail only when using the skill safely would otherwise be impractical.
