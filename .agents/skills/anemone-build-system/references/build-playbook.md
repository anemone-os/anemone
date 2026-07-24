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
| Initialize or reset local KernelConfig | `just defconfig` | Justfile, `conf/.defconfig`, existing root `kconfig` |
| List targets | `just conf ...` | target and Platform files |
| Build the kernel | `just build --preset ...` or the complete low-level tuple | selected KernelConfig, target, Platform, build task |
| Format Rust | `just fmt <scope> ...` | explicit `all`, `kernel`, or app scope; fmt help and task |
| Build an app | `just app ...` or the app xtask interface | app manifest, app task, selected architecture |
| Materialize a rootfs | `just rootfs ...` or the rootfs xtask interface | rootfs manifest, host inputs, app manifests, rootfs task |
| Run QEMU | `just qemu ...` | explicit selection for automation, selected Platform bind declarations, firmware/device/image inputs |
| Clean outputs | `just clean` | live recipe and cleanup task |
| Run pretest/end-to-end validation | existing architecture-specific repository wrapper | the entire wrapper, local prerequisites, requested validation scope |

Use `--help` to obtain current arguments instead of copying detailed invocations from this reference.

## Verify By Task

### Configuration

- Confirm whether the command creates or overwrites root `kconfig`.
- Resolve the explicitly selected target and Platform through the shared resolver.
- Reject bare, partial, or mixed preset/tuple input instead of filling it from local state.

### Kernel Build

- Confirm which selection source, SystemTarget, Platform, KernelConfig, and kernel Cargo profile were resolved.
- Check generated inputs before interpreting compiler failures.
- Provide every binding referenced by selected QEMU provider fields; build rejects runtime-only or otherwise
  unconsumed values before side effects.
- Check that the generated boot definition matches the selected initial-program variant. For `EmbeddedApp`, verify
  the app identity, single executable regular export, reported byte count, and `include_bytes!` dependency all come
  from the current invocation rather than a stale `build/apps/` artifact.
- For firmware delivery, verify normal build removes stale `build/generated/device-tree/platform.dtb`.
  For embedded delivery, verify it compiled the physical normative DTS or dumped a build-local DTB
  from the selected QEMU provider using topology fields only.
- Verify the kernel ELF and every post-link output required by the selected Platform.
- Treat an existing artifact as evidence only when its timestamp and provenance match the invocation.

### App Build

- Confirm the CLI app name locates the intended manifest.
- Confirm requested architecture, closed Cargo/Source driver, and declared artifact path agree.
- Treat Source as a command no-op only: reject manifest or caller driver args, then retain the same
  path expansion, ordinary-file check, export, and explicit post-export diagnostics as Cargo.
- Inspect exported artifacts under `build/`; use app-local target output only for diagnosis.

### Rootfs

- Validate the manifest's base tree, declared host files, and app inputs before execution.
- Confirm `fs.type` is explicit. Folder images always use automatic sizing; there is no manifest
  capacity policy.
- Confirm architecture and installed artifacts agree with the intended kernel/platform.
- When a recipe consumes a fixed repository output, run the documented producer action first and stop if it fails; path existence alone is not freshness evidence.
- Determine the exact output directory that will be replaced.
- Separate staging failures from host image-tool or privilege failures.

### QEMU

- Confirm the kernel was built for the selected platform.
- Validate the CPU, optional BIOS, firmware and fixed device tokens from the live Platform file, then provide every
  provider/fixed-arg binding and every required runtime group. Optional groups may be omitted. Values are opaque;
  xtask does not validate paths, units, ranges, or QEMU keyval semantics. Omitted BIOS emits no `-bios`.
- Distinguish launch/configuration failures from guest boot failures.
- When DTB is involved, inspect both the build-time generator and runtime QEMU path rather than assuming they use identical inputs.
- QEMU exposes no DT maintenance command. When embedded QEMU DT materialization is involved, inspect
  the normal build command and output publication; it may consume provider-field bindings but not ordinary args or
  runtime groups.

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
