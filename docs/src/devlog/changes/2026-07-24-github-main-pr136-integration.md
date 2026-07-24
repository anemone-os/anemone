# ANE-CHG-20260724-github-main-pr136-integration

**Type:** Integration
**Status:** Completed
**Date:** 2026-07-24
**Authors:** doruche, Codex
**Area:** LoongArch / Loongson 2K1000 / AHCI / build system / task lifetime / KUnit

## Problem

`github/main@9f962839` 通过 PR #136 带入 Loongson 2K1000 platform、AHCI ATA block driver、
LoongArch 修正、单 CPU KUnit 兼容、task lifetime 修复与 TID policy。接收分支
`dev/drc/alpha@5571b590` 已经完成 System Target Model R3、静态设备号 ownership 与 TTY
subsystem，因而直接采用上游文本会同时恢复旧 build model、anonymous simple stdin、dynamic block
driver/devnum 以及仓库内生成 DTB；这些都是 Git 文本冲突之外的 owner/语义冲突。

两个物理板 rootfs 还各自留下一个没有可移植获取方式的 xv6 gitlink/build 注释。仓库已经有 tracked
`xref` 外部源码注册表，继续保留 orphan gitlink 或再引入 xv6 submodule 会制造第二份 revision/fetch
authority，并把参考源码误接成 rootfs build input。

## Scope

本轮只完成 PR #136 的集成适配：

- 保留 2K1000 machine/interrupt/platform discovery、LoongArch 修正与 DTS；
- 保留 AHCI driver，并接入当前静态 block devnum、producer-owned name/minor 与 registration API；
- 保留单 CPU KUnit、kthread/signal task lifetime 与 TID reuse policy 改动；
- 把 2K1000 target/preset/rootfs/DTB/post-link output 接入当前显式 SystemTarget build model；
- 保留上游 AHCI RFC、transaction 与 register 页面，但不在本次 merge 中重新裁决或同步其 lifecycle；
- 审计并排除 simple stdin、生成 DTB、orphan xv6 gitlink及其隐含构建依赖。

本轮不补做 AHCI lifecycle/completeness，不新增 AHCI 能力，不运行 LoongArch runtime/实机，不修改 TTY
contract，也不把 xv6 变成 Anemone 构建依赖。

## Solution

alpha 以保留双亲的 no-ff merge 接收 `github/main`。当前 owner 优先于上游旧形状：console/boot stdio
继续使用 TTY subsystem 已生效的 `InitStdio` handoff；AHCI 只作为 SCSI major 8 的 block producer
提交 `BlockDevRegistration { name, device }`；build 继续由 explicit preset -> SystemTarget -> Platform
解析，不恢复旧的 build command 或默认选择。

2K1000 只跟踪 normative DTS。普通 kernel build 通过现有 Platform DT contract 生成
`build/generated/device-tree/platform.dtb` 并嵌入 kernel；本次不新增板级 `.dtb` 或 kernel-local
`generated.dtb`（既有 device-tree parser test fixture 不属于本次平台产物）。2K raw U-Boot output
只做一次 `objcopy`，不沿用 VisionFive 的 `mkimage` 路线。

xv6 不引入 submodule。`xref/sources.toml` 已经以 immutable commit 登记 xv6-riscv，并由
`just xref fetch/check` 物化 ignored reference checkout；它只服务源码对照，不是 contract、rootfs
payload 或普通 build 的网络依赖。两个 rootfs 中的 orphan gitlink与已注释 xv6 build snippet删除。
如果未来确实要在 guest 中构建或安装 xv6，应另行定义显式、可复现的 app/rootfs artifact input，不能
复用 reference-only xref checkout。

## Change

- 新增 Loongson 2K1000 machine descriptor、IPI/interrupt controller、platform discovery、DTS、
  `2k1000-la64` Platform/SystemTarget/release preset与rootfs composition。
- 平台拥有 early console register：QEMU LA64为`0x1fe001e0`，2K1000为`0x1fe20000`；build
  schema/resolver/generator按当前 config model接入AHCI Kconfig与Platform outputs。
- AHCI 保留 generic platform probe、ATA identity与同步block I/O实现，但移除上游dynamic
  `BlockDriver`/major/name ownership；driver-local allocator生成`sda`、`sdz`、`sdaa`等名称并使用
  静态SCSI major 8。
- 保留kthread entry与signal delivery的`Arc<Task>` lifetime修复、单CPU KUnit边界和可配置TID reuse；
  signal修复迁入当前`task/sig/delivery.rs` owner。
- 删除上游simple stdin及其专用ring-buffer API、anonymous console stdin接线、IRQ-off IPI/deferred
  disposal诊断、旧flat NS16550A路径、旧build implementation、上游对IDE target配置的改动和`CLAUDE.md`
  symlink。
- 删除`anemone-kernel/src/generated.dtb`、`conf/platforms/2k1000-board.dtb`和两个rootfs的xv6
  gitlink/build注释；VisionFive当前rootfs/build配置保持alpha owner版本。
- 上游AHCI RFC/transaction/register的语义和lifecycle状态按上游接入，仅规范化Markdown空白；其文档
  状态已知可能滞后，但依照本轮边界不做completion判断或额外修订。

## Validation

- `just xtask-test`：73 passed / 0 failed；`just fmt all --check`、worktree/index
  `git diff --check`、Platform schema JSON parse与duplicate-key rejection、`mdbook build docs`通过。
- `qemu-virt-rv64-pretest-release`、`qemu-virt-la64-pretest-release`、
  `visionfive2-rv64-release`和`2k1000-la64-release`四个preset均完成compile/link。RV64首次在sandbox
  内被lwext4 C compile的seccomp SIGSYS拦截，使用相同命令在sandbox外重跑通过；其余构建直接使用
  同一环境。DTC只报告各tracked DTS的既有warning。
- 2K1000 build从committed DTS生成`build/generated/device-tree/platform.dtb`，raw image只执行一次
  `rust-objcopy`到`build/anemoneImage-la64-raw`且没有`mkimage`。
- 按用户边界不运行LA64 runtime、2K1000实机、AHCI sector I/O、shutdown/reboot或完整LTP；build
  证据不外推为runtime/hardware完成度。

## Tracking Issues

### CHG-001 - anonymous simple stdin 与当前 TTY形成并列owner

**Status:** Neutralized
**Severity:** Keter

**Issue:** 直接合并会让anonymous console stdin与已生效TTY endpoint/boot stdio同时处理输入、canonical
editing和read wakeup。

**Resolution:** simple stdin、其ring-buffer专用API和boot接线全部排除；production boot继续只消费TTY
准备的`InitStdio`。

### CHG-002 - rootfs xv6 gitlink缺少可移植revision/fetch owner

**Status:** Neutralized
**Severity:** Keter

**Issue:** 两个物理板rootfs中的gitlink没有正常submodule声明；引入submodule又会与tracked xref registry
重复拥有同一外部源码，并把reference checkout耦合进rootfs。

**Resolution:** 删除两个gitlink和死build注释，不新增submodule。公共xv6参考继续只由
`xref:xv6-riscv-20260717`拥有；普通build不读取xref。

### CHG-003 - 上游AHCI lifecycle文档状态可能滞后

**Status:** Deferred
**Severity:** Safe

**Issue:** PR携带的RFC/transaction/register状态与维护者对AHCI完成度的判断可能不同。

**Resolution:** 本轮不以这些状态阻塞driver/platform合并，也不猜测更新completion；页面按上游事实接入，
后续若要同步lifecycle，应作为独立docs/change closure进行。

## Risk / Follow-up

- LA64只做compile/link；runtime、hardware与AHCI实际I/O均为Not Run。
- AHCI completeness和其文档lifecycle明确不属于本次merge；本记录不把继承页面中的Review Hold重新确认为
  当前技术判断。
- xref与rootfs artifact职责保持分离；未来guest xv6需求必须先定义明确producer、pinned input和离线/网络边界。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contracts: [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md),
  [`TTY-ENDPOINT-001`](../../contracts/tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction)
- Related small changes: [device devnum ownership](./2026-07-22-device-devnum-ownership.md),
  [xref external source registry](./2026-07-24-xref-source-registry.md)
- RFC / transaction: [AHCI Controller](../../rfcs/ahci-controller/index.md),
  [AHCI Controller transaction](../transactions/2026-07-23-ahci-controller.md)
- Issue / PR: [anemone-os/anemone#136](https://github.com/anemone-os/anemone/pull/136)
