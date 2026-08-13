# ANE-CHG-20260813-la64-runtime-fdt

**Type:** Small Feature / LoongArch64 boot discovery
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** LoongArch64 bootstrap / device tree / Platform configuration / QEMU build

## Problem / Context

比赛初赛与决赛分别用 1 CPU/1 GiB 和 8 CPU/8 GiB 启动同一个 LA64 kernel ELF。此前临时实现让 build
生成两份固定 DTB，再由 kernel 通过 QEMU `fw_cfg` 读取 CPU/内存并选择其一。这把运行时硬件拓扑同时冻结在
Platform build 配置、两份 DTB 和 selector 中，只覆盖两个测试 tuple；拓扑变化时需要重新编译或继续增加特判。

根因不是“kernel ELF 必须在同一架构的任意平台通用”，而是同一 QEMU platform 的 discoverable CPU、内存与设备
拓扑被误当成了编译时 kernel policy。QEMU direct boot 本来按照 LoongArch boot ABI 在 `a2` 交付一个
EFI-system-table-shaped envelope，并以 `DEVICE_TREE_GUID` 发布本次启动的 FDT；kernel 没有消费这个既有发现
协议，才被迫制造第二份拓扑真相。

## Decision

- `qemu-virt-la64` 使用 `dtb.delivery = firmware`。LA64 early entry 保留 `a2`，最小解码 system table 与
  configuration table，只取 `DEVICE_TREE_GUID` 指向的 FDT；不提供 UEFI service、ACPI 或通用 boot-protocol
  operation table。
- `2k1000-la64` 继续使用 normative DTS 加 embedded DTB。物理板裸机入口不需要 EFI envelope，现有启动行为不变。
- Platform 只在 build 时选择 `Embedded(bytes)` 或 `Firmware` delivery；两条路径随后统一交给既有
  machine、CPU、时钟、内存和设备树发现 consumer。MachineDesc 不解析启动 ABI，避免“先需 FDT 选择 machine、
  又需 machine 取得 FDT”的循环。
- firmware FDT 位于 kernel image 外的普通 RAM。early memory scanner 在第一次 early allocation 前按完整页范围
  将其标记为 FDT reserved；当前 PMM 没有 early reserved-zone reclaim，因此本次启动保留这些页，即使
  `unflatten_device_tree()` 随后已复制内容。
- firmware-delivery build 不消费 `smp`、`memory` 或其它 QEMU bind；这些值只属于 QEMU launch。只有 embedded
  QEMU DT materialization 才消费其 provider fields 引用的 build bind。

## Implementation Boundary

**Target:** 同一个 competition LA64 ELF 在同一个 QEMU virt platform 上从 runtime FDT 发现不同 CPU、内存和设备
配置，替换 contest-only 双 DTB/fw_cfg selector；同时保持 2K1000 embedded-DTB 启动协议。

**Owners / handoff:** Platform 配置唯一选择 FDT delivery；LoongArch bootstrap 唯一解码该架构的 firmware
boot envelope，并向既有 open-firmware consumer 交付一个 validated FDT virtual address；FDT 继续唯一拥有
machine、CPU、memory 和 device topology。xtask build 只为 embedded delivery materialize DTB，QEMU launch
唯一拥有 runtime topology bind。

**Failure / cleanup:** firmware delivery 缺少 `a2` system table、signature 无效、configuration table
地址/算术溢出、缺少或提供空 FDT 都在 early boot fail stop，不静默回退到编译时 DTB。external FDT 在
allocator 可见前被 reserve，避免被早期分配覆盖；本轮不引入可撤销 boot state 或 cleanup callback。

**Protected surface / Contract Impact:** 保持 RV64 firmware FDT、2K1000 embedded FDT、kernel ELF/public Rust
API、userspace ABI、SystemTarget/root/initial-program、QEMU runtime bind 与 current contracts。配置 current
contract 没有定义 DT delivery 或 LoongArch boot ABI，本轮 `Contract Impact` 为 `None`。不承诺同一个 ELF
跨 QEMU virt 与 2K1000，也不引入跨架构/跨 firmware 的统一启动协议抽象。

**Acceptance / stop conditions:** competition LA64 kernel 必须只编译一次，同一 ELF 分别在初赛 1C/1G 与决赛
8C/8G harness 进入并实际执行负载；2K1000 仍能构建 embedded DTB。若需要改变 boot ABI、MachineDesc owner、shared
contract、引入完整 EFI/ACPI services 或让同一 ELF 跨物理/QEMU platform，则停止并升级 RFC。

## Change

- 新增 owner-local `arch/loongarch64/boot_params.rs`，按 LoongArch/QEMU 已交付的最小 EFI table layout 查找
  runtime FDT；bootstrap assembly 在复用参数寄存器前保存 `a2`，只向 BSP discovery path 交付该值。
- generated Platform definitions 直接生成 `PlatformFdt::{Embedded,Firmware}`；删除 submission Platform、
  alternate DTB、fw_cfg topology reader、tuple parser 与双 DTB generated definitions。
- external FDT 在 `EarlyMemoryScanner` 中 reserve；后续仍复用同一 early scan、machine select 与 unflatten
  路径。
- QEMU LA64 与 competition target 切到 generic firmware-delivery Platform；Makefile、build task 和 live wrappers
  不再向 firmware build 传 runtime topology bind，QEMU invocation 保留原 bind。

## Validation

- `just test xtask`：89/89 通过；新增覆盖 LA64 QEMU firmware 与 2K1000 embedded generated definitions，
  以及 firmware build 不要求且拒绝 runtime topology bind。
- `just build --preset competition-final-la64-release` 通过；generated `PLATFORM_FDT` 为 `Firmware`，且不存在
  build-time `build/generated/device-tree/platform.dtb`。
- `just build --preset 2k1000-la64-release` 通过；generated `PLATFORM_FDT` 为 `Embedded(...)`，约 16 KiB DTB
  与 raw board image 均生成。`just build --preset competition-final-rv64-release` 通过，firmware delivery 同样不遗留
  build-time DTB。
- 在赛方 Docker `docker.1panel.live/zhouzhouyi/os-contest:20260510` 中以 root、QEMU 10.0.2 执行一次
  `make kernel-la`，生成唯一的 topology-neutral ELF（7,749,816 bytes，SHA256
  `083a5fd2f1fe5d66019d60a0bb9f8247567610667c5e5d586725f5f65adaf7dd`）。
- 同一 ELF 在 1C/1G 下识别 frozen preliminary disk、完成环境初始化并进入 LTP；已观察多个 group/case 实际执行后
  人工终止。该证据不表示完整 preliminary workload 通过。
- 同一 ELF 在 8C/8G 下识别 frozen final disk；`cagent-glibc` 十项均通过，`cagent_testcode.sh` 报告 passed，
  并进入 `buildstorm-glibc` 后人工终止。该证据不表示完整 final workload 通过。
- `just fmt all --check`、`mdbook build docs` 与 `git diff --check` 通过。
- 独立 change review 未发现 Apollyon/Keter 或 blocking finding；修正了英文 README 中残留的 build-time topology
  bind 示例与证据 provenance 措辞。residual risks 为 2K1000 实体板 Not Run、比赛负载均人工终止，以及 malformed
  EFI/FDT fail-stop 路径没有 focused runtime/unit coverage。

## Remaining Risk / Links

- 本轮只消费 LoongArch direct-boot ABI 已提供的 FDT configuration-table entry。完整 EFI services、ACPI、多种
  firmware protocol negotiation 或跨 Platform universal kernel 都没有真实 consumer，不能由这次局部修复推导为
  已接受方向。
- 2K1000 实体板 runtime 为 Not Run；其 embedded delivery 由独立 build 保持。
- Current contract / RFC / transaction / register：None。
- 外部依据：Linux 6.6.32 LoongArch boot documentation 与 source；QEMU 10.0.2 runtime behavior。
