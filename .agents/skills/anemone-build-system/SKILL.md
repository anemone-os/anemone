---
name: anemone-build-system
description: Use when building, cleaning, configuring, formatting, packaging rootfs images, building apps, running QEMU, auditing Anemone build configuration, handling DTB generation, or using repository pretest/end-to-end flows. Route build-facing work through the Justfile, scripts/xtask, and existing repository wrappers; inspect live owners before acting and avoid bare cargo or ad-hoc target commands that bypass repository orchestration.
---

# Anemone Build System

## Preserve The Build Contract

1. Work from the repository root.
2. Use `just ...` for common flows, `just xtask ...` for specific xtask interfaces, and existing repository wrappers for their complete end-to-end flows.
3. Do not substitute bare `cargo`, `rustc`, formatter, linker, target, or cleanup commands for repository-owned orchestration. Xtask owns generated inputs, explicit target selection, artifact export, and platform wiring.
4. Inspect user-facing exports under `build/` first. Treat cargo `target/` trees as internal unless diagnosis requires them.
5. Change the Justfile or `scripts/xtask/` when orchestration is the owner. Do not add a parallel build entrypoint or one-off wrapper.

## Follow The Live Workflow

For every build-facing task:

1. Classify it as configuration, kernel build, app build, rootfs, formatting, QEMU, cleanup, or end-to-end validation.
2. Discover the current interface with `just --list` or `just xtask <command> --help`.
3. Read the active configuration and the owning code under `scripts/xtask/src/config/` and `scripts/xtask/src/tasks/`.
4. Identify prerequisites, outputs, overwritten state, and deletion scope before executing.
5. Choose the narrowest repository entrypoint that satisfies the request.
6. Verify the outputs promised by the active configuration and command; do not infer success from unrelated or stale files.

Read [references/build-playbook.md](references/build-playbook.md) for task routing and staged diagnosis. Read [references/config-model.md](references/config-model.md) when changing or auditing configuration relationships.

## Respect Configuration Owners

Keep each concern in its owning layer:

- root `kconfig` and `conf/.defconfig`: kernel features, policy, and capacity only;
- `conf/build-presets/`: reusable explicit target, KernelConfig, and kernel Cargo-profile tuples;
- `conf/system-targets/`: selected Platform reference, root mount/source, and initial-program source;
- `conf/platforms/` and `conf/arch/`: platform identity, architecture, hardware constants, boot environment, tracked QEMU argv/bind templates, DTB, linker inputs, and Platform-required kernel outputs;
- `anemone-apps/<app>/app.toml`: closed Cargo/Source driver and exported artifacts; Source runs no
  command and only admits existing ordinary files through the common export path;
- `conf/rootfs/`: rootfs composition and installed apps/files;
- Justfile and `scripts/xtask/src/tasks/`: orchestration and command behavior.

Kernel build owns the generated initial-program input. `RootfsEntry` emits its typed tag and optional complete argv;
`EmbeddedApp` resolves the referenced app through the same architecture-specific `build_app()` exporter used by
ordinary app/rootfs actions, requires exactly one executable regular artifact, and emits an ignored typed Rust
definition whose `include_bytes!` points at that export and which carries the same optional argv shape. Explicit argv
includes argv[0]; omission uses the resolved executable path as the sole argument. The kernel must not parse SystemTarget or app manifests,
and `clean` must remove the generated boot definition.

Build and ordinary QEMU require an explicit `--preset` or a complete `--target` / `--kernel-config`
/ `--profile` tuple. Bare invocation has no local or repository-default fallback. Build/QEMU bind values are opaque
action inputs supplied as `--bind name=value`, not tracked configuration. Provider-field placeholders are consumed by
build and QEMU; fixed QEMU args and required/optional argv groups are QEMU-only.

Formatting also requires an explicit scope: `all`, `kernel`, or an app name.
Rootfs manifests require an explicit filesystem base type; folder roots use the repository's single
automatic sizing policy rather than a manifest capacity option. QEMU Platforms require an explicit
CPU model, while an omitted BIOS means xtask emits no `-bios` argument.

QEMU DT has no maintenance or source write-back action. Firmware delivery consumes the runtime FDT;
embedded QEMU delivery is materialized only by normal build from the selected provider's resolved machine,
CPU, SMP, memory, and optional BIOS. Build may consume bindings referenced by those provider fields, but must not
consume ordinary QEMU args, runtime bind groups, rootfs,
disks, or network backends for that dump. QEMU Platforms keep no committed DTS mirror. Physical
Platforms may retain a normative source or firmware conformance baseline; baseline provenance,
allowed differences, and revalidation responsibility remain human-reviewed facts rather than
machine-maintained fields without a real consumer.

When prose, examples, schemas, active configuration, and Rust code disagree, treat live deserialization and task code as authoritative. Re-read them instead of preserving a possibly stale fact in this skill.

## Protect Generated State

Do not hand-edit generated kernel definitions, linker outputs, DTB outputs, or exported artifacts. Regenerate them through their owning command.

Before cleanup, configuration reset, disk creation, rootfs materialization, or an end-to-end wrapper, inspect the live recipe or script and report any relevant state it overwrites or deletes. Do not run a broad flow as a substitute for a narrower validation.

Generated outputs can be conditional and old files can survive a later command. Verify both provenance and freshness when a result is used as evidence.

## Diagnose In Ownership Order

1. Preserve the exact repository command and read its first actionable failure.
2. Check the selected configuration and cross-layer relationships.
3. Check expected exports under `build/`, including freshness when applicable.
4. Inspect the owning config type and task implementation.
5. Fix the owning layer only; do not hide configuration mismatches with manual copies or ad-hoc shell commands.

Keep the skill durable: record stable owner boundaries and verification procedures here, while deriving checkout-specific paths, flags, defaults, and platform wiring from current source.
