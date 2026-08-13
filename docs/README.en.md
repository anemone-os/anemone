![Harbin Institute of Technology, Shenzhen](../report/kernel-report/assets/school.jpg)

# Anemone

[![CI](https://github.com/anemone-os/anemone/actions/workflows/ci.yml/badge.svg)](https://github.com/anemone-os/anemone/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Language: Rust](https://img.shields.io/badge/language-Rust-dea584.svg?logo=rust)](https://www.rust-lang.org/)
[![Architectures: RISC-V64 & LoongArch64](https://img.shields.io/badge/architectures-RISC--V64%20%7C%20LoongArch64-5c6bc0.svg)](#multi-architecture-without-two-forked-kernels)

English | [简体中文](../README.md)

> Linux UAPI at the boundary, Anemone's own order within the kernel.

Anemone is a multicore Unix-like monolithic kernel written in Rust, with support for RISC-V64 and LoongArch64. It aims to run Linux userspace without copying Linux's internal object model: processes, scheduling, virtual memory, VFS, devices, sockets, and network protocols remain owned by focused Anemone subsystems.

From QEMU to VisionFive 2 and Loongson 2K1000, and from syscall suites to interactive shells, network tools, and large compilation workloads, Anemone is built around a simple proposition: Linux compatibility, cross-architecture portability, and maintainable system design do not have to be traded against one another.

<img src="../report/kernel-report/assets/anemone-architecture.png" alt="Anemone architecture" width="1000"/>

## Why Anemone?

### Linux compatibility at the ABI boundary

Anemone's syscall layer decodes the Linux ABI, accesses userspace memory, and maps operations onto kernel objects. Core subsystems retain their own state machines, lifecycles, and failure boundaries. Compatibility therefore comes from composable mechanisms rather than test-specific branches scattered across the kernel.

The same principle appears throughout the system. The scheduler uses its own wait state machine for blocking, wakeup, and signal races. VMOs provide a common foundation for anonymous memory, file mappings, copy-on-write, and shared backing. The VFS distinguishes paths, inodes, opened file descriptions, and concrete backends. A common Socket frontend exposes Linux file semantics while TCP, UDP, ICMP raw, Unix sockets, and Netlink retain protocol-specific state.

### Multi-architecture without two forked kernels

The memory, exception, scheduling, and signal subsystems define the architecture capabilities they consume; RISC-V64 and LoongArch64 implement those capabilities. Architecture code owns hardware facts such as page-table formats, trap frames, context switches, interrupts, and instructions, but not higher-level policy.

The same core kernel currently covers:

- RISC-V64 and LoongArch64;
- QEMU virt machines;
- VisionFive 2 and Loongson 2K1000 hardware platforms;
- VirtIO MMIO, VirtIO PCIe, SD card, and AHCI device paths.

### State ownership as a first-class design rule

The hardest kernel problems are rarely isolated syscalls. They arise when concurrent operations disagree about who owns the current fact, who advances a transition, and who cleans up after failure. Anemone aims to give each class of state one owner and to cross module boundaries using narrow capabilities, tokens, snapshots, and recheck notifications instead of shared private objects or duplicate sources of truth.

The network stack is a compact example. Linux sockets, protocol endpoints, the protocol engine, and NIC resources live at separate layers; smoltcp handles, buffers, and private state do not leak into kernel Socket objects. Likewise, the device model separates device discovery and driver binding from VFS file interfaces, allowing virtual and physical devices to share the same block and filesystem layers.

### A complete system, not only a kernel ELF

Anemone keeps the kernel, shared ABI, userspace libraries, applications, root filesystems, platform descriptions, and build configurations in one repository. `Justfile` and the Rust-based `xtask` turn an explicit system description into bootable artifacts.

The system can host an interactive shell, a glibc userspace, process and file utilities, `ping`, native iproute2, `apt install`, HTTPS `git clone`, and large software builds. The competition finals and BuildStorm compilation workloads exercise how these mechanisms work together along complete paths rather than merely checking that isolated interfaces exist.

## Capability overview

- **Processes and scheduling:** task, thread-group, process-group, and session lifecycles; signals and job control; Fair, FIFO, RR, and Stride scheduling classes; multicore load balancing; and a shared wait core.
- **Memory management:** physical pages and page tables, address spaces, VMOs, demand paging, COW, shared memory, file-backed mappings, page cache, TLB shootdown, and OOM protection.
- **Files and devices:** VFS, mount tree, Ext4, tmpfs, procfs, devfs, a common opened-file model, and a bus/device/driver framework.
- **IPC and events:** signals, pipes, System V IPC, eventfd, timerfd, poll/select/epoll, and Unix sockets.
- **Networking:** IPv4 TCP, UDP, ICMP raw, VirtIO-Net, Socket readiness, read-only Netlink diagnostics, and native userspace network tooling.
- **Time and architecture:** clocks, ticks, timers, RTC, SMP boot, traps, interrupts, IPIs, context and signal frames, and architecture-specific userspace interfaces.
- **Build and validation:** typed KernelConfig, Platform, SystemTarget, and BuildPreset inputs; dual-architecture builds; rootfs generation; QEMU and board artifacts; and end-to-end test entry points.

See the [kernel technical report](../report/kernel-report/) for the full design and the [active register](./src/register.md) for current open issues and accepted limitations.

## Results and materials

At the point the current finals report was completed, Anemone had passed all competition-final test cases and achieved competitive results in the BuildStorm compilation workload. The ranking image is a snapshot of the corresponding competition result; the reports and developer documentation define the exact validation scope and boundaries.

<img src="../report/kernel-report/assets/final-rank.png" alt="Anemone finals ranking snapshot" width="720"/>

- [Kernel technical report](../report/kernel-report/) (Typst sources)
- [BuildStorm design and optimization report](../report/buildstorm/)
- Finals presentation: coming soon
- Finals demo video: coming soon

## Building with `just`

The recommended environment is the repository's development container, or a host with an equivalent Rust toolchain, cross toolchains, and QEMU installation. Build and run operations should enter through `Justfile` / `scripts/xtask`; these commands resolve the system target, generate configuration and linker inputs, and publish the final artifacts.

```sh
# Discover commands and tracked configurations
just --list
just conf list

# Build RISC-V64 and LoongArch64 QEMU release kernels
just build --preset qemu-virt-rv64-release
just build --preset qemu-virt-la64-release
```

The default kernel artifact is published as `build/anemone.elf`. Root filesystems, QEMU runs, physical boards, and competition end-to-end reproduction require explicit platform and disk inputs. See the [build-system guide](../scripts/xtask/README.md), [configuration guide](../conf/README.md), and [end-to-end scripts](../scripts/README.md).

## Repository layout

```text
.
├── anemone-kernel/     # Kernel and standalone kernel-side crates
├── anemone-abi/        # ABI shared by the kernel and userspace
├── anemone-rs/         # Rust userspace support library
├── anemone-libc/       # C userspace support library
├── anemone-apps/       # init, tests, tools, and example programs
├── conf/               # KernelConfig, platforms, targets, presets, and rootfs
├── scripts/            # xtask, build/run commands, and end-to-end validation
├── docs/               # Current contracts, RFCs, development records, register
├── report/             # Technical, optimization, and presentation materials
├── xref/               # Pinned external implementation references
├── Cargo.toml
└── Justfile
```

## Developer documentation

[`docs/`](./) contains the current engineering knowledge for the project, including:

- [Development workflow](./src/development-workflow.md)
- [Current contracts](./src/contracts.md)
- [RFCs and major design decisions](./src/rfcs.md)
- [Development records](./src/development-log.md)
- [Open issues and current limitations](./src/register.md)

This README is an entry point, not a second source of truth. The current source and the corresponding documentation define capability boundaries, known gaps, and validation status.

## Team

Harbin Institute of Technology, Shenzhen:

- Zhenghan Zhang: process management, memory management, filesystems, IPC, networking, device drivers, RISC-V port, time, syscalls, documentation, and test support.
- Hanshen Chen: PCIe, process management, memory management, LoongArch port, physical-board enablement, and test support.
- Advisors: Wen Xia and Jieting Qiu.

## License

Except for third-party code noted otherwise, Anemone is dual-licensed under the [MIT License](../LICENSE-MIT) or the [Apache License 2.0](../LICENSE-APACHE).
