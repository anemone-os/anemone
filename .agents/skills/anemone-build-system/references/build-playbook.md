# Build Playbook

Use this reference to route a task without freezing checkout-specific configuration into the skill.

## Discover Before Executing

Start with the live interface:

```text
just --list
just xtask <command> --help
```

Then read the corresponding Justfile recipe and xtask task. Help output defines the current CLI; task code defines side effects and outputs.

## Route The Task

| Task | Preferred entrypoint | Inspect before use |
| --- | --- | --- |
| Initialize or reset local build configuration | `just defconfig` | Justfile, `conf/.defconfig`, existing root `kconfig` |
| List or switch platforms | `just conf ...` | conf task, root `kconfig`, platform files |
| Build the kernel | `just build` or the build xtask interface | selected kconfig, platform, build task |
| Format Rust | `just fmt ...` | fmt help and fmt task |
| Build an app | `just app ...` or the app xtask interface | app manifest, app task, selected architecture |
| Materialize a rootfs | `just rootfs ...` or the rootfs xtask interface | rootfs manifest, host inputs, app manifests, rootfs task |
| Run QEMU | qemu xtask interface | platform, kernel provenance, firmware/device/image inputs |
| Clean outputs | `just clean`, `just mrproper`, or `just xtask-clean` | live recipe and cleanup task |
| Run pretest/end-to-end validation | existing architecture-specific repository wrapper | the entire wrapper, local prerequisites, requested validation scope |

Use `--help` to obtain current arguments instead of copying detailed invocations from this reference.

## Verify By Task

### Configuration

- Confirm whether the command creates, overwrites, or edits root `kconfig`.
- Re-read the selected platform after a switch.
- Separate local configuration from tracked defaults.

### Kernel Build

- Confirm which selection source, SystemTarget, platform, KernelConfig, and kernel Cargo profile were resolved.
- Check generated inputs before interpreting compiler failures.
- Verify the kernel ELF and every post-link output required by the selected Platform.
- Treat an existing artifact as evidence only when its timestamp and provenance match the invocation.

### App Build

- Confirm the CLI app name locates the intended manifest.
- Confirm requested architecture, driver profile, and declared artifact path agree.
- Inspect exported artifacts under `build/`; use app-local target output only for diagnosis.

### Rootfs

- Validate the manifest's base tree, declared host files, and app inputs before execution.
- Confirm architecture and installed artifacts agree with the intended kernel/platform.
- When a recipe consumes a fixed repository output, run the documented producer action first and stop if it fails; path existence alone is not freshness evidence.
- Determine the exact output directory that will be replaced.
- Separate staging failures from host image-tool or privilege failures.

### QEMU

- Confirm the kernel was built for the selected platform.
- Validate firmware, device, disk, and image inputs from the live platform file.
- Distinguish launch/configuration failures from guest boot failures.
- When DTB is involved, inspect both the build-time generator and runtime QEMU path rather than assuming they use identical inputs.

### Cleanup And End-To-End Wrappers

- Read the live recipe or wrapper completely before running it.
- Summarize configuration changes, deletions, overwritten staging files, privilege use, runtime launch, and log destinations relevant to the user.
- Do not use an end-to-end wrapper when a build, format check, rootfs-only check, or command inspection is sufficient.

## Diagnose By Stage

1. **Interface:** verify command family and arguments from live help.
2. **Configuration:** verify parsing, selection, and cross-layer consistency.
3. **Generation:** verify generated definitions, linker inputs, and DTB handling.
4. **Compilation/export:** separate compilation from artifact export.
5. **Rootfs:** separate input staging from image materialization.
6. **QEMU:** separate host launch from guest behavior.
7. **Wrapper:** use its progress/log boundary to isolate the failed inner stage before rerunning the whole chain.

Keep fixes at the first failing owner boundary. Do not bypass the repository flow merely to make a later stage run.
