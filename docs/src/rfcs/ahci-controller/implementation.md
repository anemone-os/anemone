# AHCI Controller 迁移与验证计划

**状态：** Active / Review Hold
**父 RFC：** [AHCI Controller / ATA Block Device](./index.md)
**不变量：** [AHCI Controller 不变量](./invariants.md)
**Tracking Issues：** [AHCI Controller Tracking Issues](./tracking-issues.md)
**事务日志：** [2026-07-23 AHCI Controller](../../devlog/transactions/2026-07-23-ahci-controller.md)

## 当前基线

实现先于公共 RFC 存在：

- `48f86615` 新增 generic AHCI platform/block driver、ATA IDENTIFY、LBA48 DMA EXT read/write、
  single-port polling controller、DMA bounce buffer、register/FIS helpers 和 focused KUnit。
- `7176098f` 把 2K1000 AHCI node 加入 `generic-ahci` fallback，增加 AHCI kconfig timeouts/bounce
  参数；后续 `d6875c69` 把模块移动到 `anemone-kernel/src/driver/ahci/`，保持行为与调用面不变。
- `just build`（当前 2K1000 LoongArch 配置，启用 `kunit`）已通过；`git diff --check` 已通过。
- `just fmt kernel --check` 仍受移动前已有的 AHCI 格式差异及工作区其他既有生成文件差异影响；本事务
  未用 formatter 覆盖用户未提交文件。
- KUnit runtime、QEMU、真实 AHCI probe/read/write/shutdown/reboot 尚未运行。

## Gate A：Lifecycle blocker neutralization

**状态：** Not Started / stop on Apollyon

**目标：** 证明任何启动 engine 后的 probe、registration 和 shutdown 路径都不会让 HBA 继续 DMA
到已释放 owner；同时拒绝超出 48-bit FIS domain 的 IDENTIFY capacity。

**最小 write set：**

```text
anemone-kernel/src/driver/ahci/{mod.rs,ata.rs,fis.rs,port.rs}
docs/src/rfcs/ahci-controller/*
docs/src/devlog/transactions/2026-07-23-ahci-controller.md
```

**验证 floor：** failure-path source audit、capacity boundary KUnit、`just build`、`git diff --check`。

**停止条件：** cleanup 需要 block registry unregister、device remove、异步 worker 或新的 DMA owner；
这类 owner surface 扩展必须先更新 RFC write set 和不变量。

**回写：** AHCI-001/AHCI-002 的修复折回 `index.md` / `invariants.md`，实际 checkpoint 追加事务日志；
若 contract 改变则建立 `R0` accepted revision，而不是只在 tracking 页标记 neutralized。

## Gate B：Controller contract and focused tests

**前置：** Gate A 的 Apollyon 已 neutralize。

**交付：**

1. 维持 generic firmware matching、MMIO bounds、DMA aperture、one-port AHCI 1.x gate。
2. 覆盖 FIS byte order、CAP N-1 fields、MMIO port window、DMA mask boundary、IDENTIFY feature/
   string/capacity rejection、interrupt priority 和 short transfer。
3. source audit 确认无 fixed-address production selection、无 second readiness owner、无 hidden
   async request path。

**验证 floor：** focused KUnit runtime（若当前 runner 可用）、`just build`、`git diff --check`；未运行
 runtime 明确记录 Not Run。

## Gate C：2K1000 hardware vertical slice

**前置：** Gate B 通过；用户提供可启动 2K1000 或等价 generic AHCI 平台和可观察串口日志。

**用户侧验证：**

1. probe 日志包含 resource、CAP/version/PI、selected port、link speed、DMA mask、model/serial/
   firmware 和 capacity。
2. 读取 LBA 0 与 last LBA，确认重复读取稳定；越界读返回错误而非 panic。
3. 写入只允许使用用户指定 disposable media 或明确 LBA，保存原内容、写入内容、readback 与恢复结果。
4. 注入或观察 link/error/short-transfer 后，确认 error log、recovery/offline 和后续拒绝行为。

**停止条件：** capacity 与设备工具不一致、silent short transfer、错误后继续 I/O、watchdog latency
 不可接受，或出现未授权写入。

## Gate D：Shutdown and close decision

**前置：** Gate C 通过。

**交付：**

- shutdown 明确 quiesce、engine stop 和 cache durability；
- runtime read timeout 不再依赖 panic，或 RFC 明确接受并给出用户可见 fail-stop 边界；
- 更新 register/current limitations、tracking issues、transaction 和双周 devlog；
- 若所有接受边界满足，将 RFC 从 Review Hold 提升为 `Accepted / Runtime Validation` 或 `Closed`；
  若硬件/生命周期 gate 失败，保持 RFC open，不把失败重命名为 limitation。

## 不应在本事务内做的事

- 不引入 IRQ worker、NCQ、multi-port scheduling、ATAPI、hotplug、partition scanner 或通用 storage
  queue。
- 不通过放宽 DMA/MMIO/capacity checks 或静默吞错来通过 gate。
- 不恢复 `driver/block/ahci` 旧路径；结构移动已经是当前 owner boundary。
