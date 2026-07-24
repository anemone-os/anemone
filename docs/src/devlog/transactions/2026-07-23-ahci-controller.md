# 2026-07-23 - AHCI Controller

**Status:** Active / Review Hold
**Owners:** EDGW, Codex
**Area:** AHCI / SATA / ATA / DMA / block / platform bus
**Canonical Plan:** [AHCI Controller / ATA Block Device RFC](../../rfcs/ahci-controller/index.md)
**Canonical Revision:** `Draft`
**Current Phase:** Baseline implementation and lifecycle review

## Scope

记录 `48f86615` 落地的 generic AHCI controller、ATA block facade、DMA/FIS/register helpers、2K1000
platform integration，以及随后把模块移动到 `driver/ahci` 的结构 checkpoint。事务不实现 IRQ/NCQ、
multi-port、ATAPI、hotplug、partition scan 或通用 storage queue。

## Invariants

- controller/port readiness、DMA owner 和 MMIO view 各只有一个 owner。
- engine/FIS receive 可能访问 DMA 时，metadata/bounce/mapping 必须保持存活。
- IDENTIFY capacity 必须能安全编码为 LBA48，block I/O 必须保持 512-byte alignment/range contract。
- 所有 command error、link loss、short transfer 和 recovery failure 都必须可观察且 fail closed。
- 当前同步 slot-zero transaction 不扩展为异步、多 port 或 IRQ-owned request lifecycle。

## Phase Log

### 2026-07-23 - Existing implementation baseline

**Phase:** Baseline
**Change:** `48f86615` 增加 `driver/block/ahci/{ata,dma,fis,mod,platform,port,regs}.rs`，实现 generic
AHCI platform match、HBA/port setup、single-slot DMA bounce buffer、ATA IDENTIFY、LBA48 DMA EXT
read/write、error classification/recovery、SCSI-class block registration 和 focused KUnit helpers。
`7176098f` 增加 2K1000 generic compatible fallback 与 AHCI kconfig timeout/bounce 参数。
**Audit:** 当前实现明确限制为 AHCI 1.x、单一 implemented port、同步 polling、slot zero、512-byte
logical sector；没有 IRQ completion、NCQ、hotplug 或 shutdown reclamation。
**Observability:** probe、slow-read、request failure 和 shutdown 路径输出 controller/port register
snapshot；model/serial/firmware/capacity 进入 probe log。
**Feedback:** 文档层 review 发现 AHCI-001/AHCI-002 两个 Apollyon，及 shutdown/read-timeout 问题；
未削弱目标或不变量，问题已写入 RFC canonical boundary 与 tracking。
**Validation:** 该提交本身没有可从 commit message 恢复的运行日志；后续 agent 在当前结构移动后的
LoongArch 配置执行 `just build` 通过，KUnit runtime、QEMU 和真实硬件 I/O 尚未运行。
**Next:** Gate A lifecycle cleanup and capacity validation。

### 2026-07-23 - Owner-boundary move

**Phase:** Structural checkpoint
**Change:** `d6875c69` 将 AHCI 文件从 `anemone-kernel/src/driver/block/ahci/` 移至
`anemone-kernel/src/driver/ahci/`；`driver/mod.rs` 直接声明 `mod ahci`，block module 不再拥有
AHCI module。文件内容和 block registration direction 保持不变。
**Audit:** 旧 `driver::block::ahci` 引用为零；未扩大 public API 或改变 ABI。用户已有的
`loongson,ls-ahci` match-table 修改随文件保留。
**Observability:** None added; this is a behavior-preserving module move。
**Feedback:** `None`；结构边界与 RFC 一致。
**Validation:** `just build` passed；`git diff --check` passed。`just fmt kernel --check` 仍报告移动前
已有 AHCI 格式差异和工作区其他生成文件差异，未用 formatter 覆盖用户未提交文件。
**Next:** Continue Gate A only after lifecycle write set is accepted.

## Open Items

- [AHCI-001](../../rfcs/ahci-controller/tracking-issues.md#ahci-001---probe-failure-can-release-live-dma-owner)：
  post-start probe failures must stop engines before DMA/MMIO release。
- [AHCI-002](../../rfcs/ahci-controller/tracking-issues.md#ahci-002---identify-capacity-can-reach-an-internal-fis-assertion)：
  reject out-of-domain IDENTIFY capacity before FIS construction。
- [AHCI-003](../../rfcs/ahci-controller/tracking-issues.md#ahci-003---shutdown-does-not-quiesce-the-controller)：
  define shutdown quiesce and cache durability semantics。
- [AHCI-005](../../rfcs/ahci-controller/tracking-issues.md#ahci-005---runtime-evidence-is-not-yet-available)：
  KUnit runtime and user hardware evidence are not run。

## Closure

事务尚未收口。当前可证明的是代码结构移动与 LoongArch kernel build；Apollyon lifecycle/capacity
问题、shutdown policy 和 hardware runtime validation 仍保持 Open/Not Run。关闭前必须更新 RFC status、
tracking issues、current limitations/register、双周 devlog 和本页最终验证记录。
