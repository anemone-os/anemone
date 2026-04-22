---
name: anemone-build-system
description: Use when building, cleaning, configuring, packaging rootfs images, building apps, or running QEMU for the Anemone repository. Route all build-facing work through Justfile and scripts/xtask, inspect outputs under build/, and avoid invoking bare cargo or ad-hoc target commands directly because xtask owns generated files, target specs, and platform-specific wiring.
---

# Anemone Build System

## Overview

Route all Anemone build, app, rootfs, configuration, and QEMU tasks through `just` or `just xtask`. Avoid bare `cargo`, `rustc`, `cargo clean`, and hand-written linker or target invocations because `xtask` injects target specs, generated Rust definitions, linker scripts, DTB generation, and staged outputs under `build/`.

## Follow These Rules

1. Start from the repository root. Prefer `just ...` for common flows and `just xtask ...` when you need a specific subcommand.
2. Initialize `kconfig` with `just defconfig` when `kconfig` is missing or when the user explicitly wants to reset to the default configuration.
3. Treat `anemone-kernel/src/kconfig_defs.rs`, `anemone-kernel/src/platform_defs.rs`, and `build/generated/kernel.lds` as generated outputs. Regenerate them via `just build` or `just xtask build`; do not edit them manually.
4. Inspect results under `build/`. Treat `target/` as cargo cache or intermediate state unless a task explicitly requires looking there.
5. Read `kconfig`, `conf/.defconfig`, `conf/platforms/*.toml`, `conf/rootfs/*.toml`, and `anemone-apps/*/app.toml` before changing build behavior.
6. Use `just clean` or `just mrproper` for cleanup. Do not run bare `cargo clean`.
7. Let `xtask` invoke cargo internally when needed. Do not bypass it by running `cargo build`, `cargo run`, or `cargo test` directly from the workspace or from `anemone-kernel` or `anemone-apps`.

## Choose The Workflow

1. Determine the task category.
	- Build the kernel or regenerate generated files -> use `just build` or `just xtask build`.
	- Switch or inspect platform configuration -> use `just conf list` or `just conf switch <platform>`.
	- Build one userspace app -> use `just xtask app build <app> --arch <arch>`.
	- Build a rootfs image -> use `just xtask rootfs mkfs -c <manifest>`.
	- Run QEMU -> use `just xtask qemu --platform <platform> --image build/anemone.elf`.
	- Clean outputs -> use `just clean` or `just mrproper`.
2. Verify prerequisites before executing.
	- Ensure `kconfig` selects the intended platform and profile.
	- Ensure the platform file under `conf/platforms/` matches the requested architecture and QEMU wiring.
	- Ensure the rootfs manifest's `[build].name` matches any hard-coded image path in the selected platform's QEMU args when booting from disk.
3. Verify outputs after executing.
	- Check `build/anemone.elf`, `build/anemone.disasm`, and `build/kernel.map` for kernel builds.
	- Check `build/apps/<app>/` for app exports.
	- Check `build/rootfs/<name>/root/` and `build/rootfs/<name>/rootfs.img` for rootfs generation.
4. Read [references/build-playbook.md](references/build-playbook.md) when you need the exact command matrix, output locations, or repo-specific caveats.

## Respect Repo Boundaries

Keep build logic in `Justfile` and `scripts/xtask/`. If a task requires changing build orchestration, edit those files instead of adding parallel shell scripts.

Keep platform constants in `conf/platforms/*.toml` and architecture templates in `conf/arch/`. Do not duplicate those constants in Rust code.

Expect `xtask build` to generate or overwrite files before compiling. Plan edits around that behavior.

Remember that `conf/platforms/qemu-virt-rv64.toml` currently points QEMU at `build/rootfs/minimal/rootfs.img`. If you change the rootfs manifest name or output layout, update the platform config or use a matching manifest.

## Diagnose Failures

1. Read the failing `just` or `xtask` command first. Do not replace it with ad-hoc cargo commands.
2. Check configuration inputs before touching code:
	- `kconfig`
	- `conf/.defconfig`
	- `conf/platforms/*.toml`
	- `conf/rootfs/*.toml`
	- `anemone-apps/<app>/app.toml`
3. Check generated outputs under `build/` next.
4. Modify `Justfile` or `scripts/xtask/src/**/*.rs` when the failure comes from orchestration, target selection, artifact export, or QEMU or rootfs wiring.

## Keep The Scope Tight

Use this skill to drive repo-local builds and build-system edits. Do not introduce a second build entrypoint, a parallel wrapper script, or cargo-only instructions that contradict the repository workflow.

