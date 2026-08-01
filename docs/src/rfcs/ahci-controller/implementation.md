# AHCI Controller 迁移与验证计划

**状态：** Terminated / Historical Plan
**父 RFC：** [AHCI Controller / ATA Block Device](./index.md)
**不变量：** [AHCI Controller 不变量](./invariants.md)
**Tracking Issues：** [AHCI Controller Tracking Issues](./tracking-issues.md)
**事务日志：** [2026-07-23 AHCI Controller](../../devlog/transactions/2026-07-23-ahci-controller.md)

> 父RFC已于2026-08-01在Draft阶段终止。Gate A-D全部取消，本文只保留当时的迁移设想和未运行验证边界，
> 不再授权任何实现、probe或hardware validation。未来AHCI工作必须作为独立任务重新分类并取得新的授权/
> Implementation Boundary，不能从本页续跑；只有重新分类仍命中RFC时才新建RFC。

## 终止时的历史基线

实现先于公共 RFC 存在：

- `48f86615` 新增 generic AHCI platform/block driver、ATA IDENTIFY、LBA48 DMA EXT read/write、
  single-port polling controller、DMA bounce buffer、register/FIS helpers 和 focused KUnit。
- `7176098f` 把 2K1000 AHCI node 加入 `generic-ahci` fallback，增加 AHCI kconfig timeouts/bounce
  参数；后续 `d6875c69` 把模块移动到 `anemone-kernel/src/driver/ahci/`，保持行为与调用面不变。
- `just build`（当前 2K1000 LoongArch 配置，启用 `kunit`）已通过；`git diff --check` 已通过。
- `just fmt kernel --check` 仍受移动前已有的 AHCI 格式差异及工作区其他既有生成文件差异影响；本事务
  未用 formatter 覆盖用户未提交文件。
- 2026-07-23原基线未运行KUnit runtime、QEMU或真实AHCI I/O。终止前本次合流的RV64/LA64完整KUnit
  runtime各自通过10个已注册AHCI helper case；`ata.rs`的IDENTIFY helper没有`#[kunit]`注册，capacity
  upper-bound regression/source audit仍未完成，真实2K1000 probe/read/write/shutdown/reboot仍Not Run。

## Gate A：Lifecycle blocker neutralization（Cancelled）

**状态：** Cancelled / Not Started

**原计划目标（未执行）：** 证明任何启动 engine 后的 probe、registration 和 shutdown 路径都不会让 HBA 继续 DMA
到已释放 owner；同时拒绝超出 48-bit FIS domain 的 IDENTIFY capacity。

**原计划路径提示：**

```text
anemone-kernel/src/driver/ahci/{mod.rs,ata.rs,fis.rs,port.rs}
docs/src/rfcs/ahci-controller/*
docs/src/devlog/transactions/2026-07-23-ahci-controller.md
```

**原验证计划（未执行）：** failure-path source audit、capacity boundary KUnit、`just build`、`git diff --check`。

**原停止设想：** cleanup若需要block registry unregister、device remove、异步worker或新的DMA owner，原计划
回到RFC review；该路线已取消，不提供当前授权。

**终止处置：** 原定向父RFC/transaction回写和建立`R0`的路线均未执行且已取消。live defect只由register
拥有；未来若另行授权，必须在独立任务的新Implementation Boundary中修复并回写register，不能修改本页以
重新激活Gate A；只有重新分类仍命中RFC时才新建RFC。

## Gate B：Controller contract and focused tests（Cancelled）

**状态：** Cancelled / Not Started

**原前置（未满足）：** Gate A的Apollyon已neutralize。

**原计划交付（未执行）：**

1. 维持 generic firmware matching、MMIO bounds、DMA aperture、one-port AHCI 1.x gate。
2. 覆盖 FIS byte order、CAP N-1 fields、MMIO port window、DMA mask boundary、IDENTIFY feature/
   string/capacity rejection、interrupt priority 和 short transfer。
3. source audit 确认无 fixed-address production selection、无 second readiness owner、无 hidden
   async request path。

**原验证计划（未执行）：** focused KUnit runtime（若当前 runner 可用）、`just build`、`git diff --check`；未运行
 runtime 明确记录 Not Run。

## Gate C：2K1000 hardware vertical slice（Cancelled）

**状态：** Cancelled / Not Started

**原前置（未满足）：** Gate B通过；用户提供可启动2K1000或等价generic AHCI平台和可观察串口日志。

**原用户侧验证计划（未执行）：**

1. probe 日志包含 resource、CAP/version/PI、selected port、link speed、DMA mask、model/serial/
   firmware 和 capacity。
2. 读取 LBA 0 与 last LBA，确认重复读取稳定；越界读返回错误而非 panic。
3. 写入只允许使用用户指定 disposable media 或明确 LBA，保存原内容、写入内容、readback 与恢复结果。
4. 注入或观察 link/error/short-transfer 后，确认 error log、recovery/offline 和后续拒绝行为。

**原停止信号：** capacity 与设备工具不一致、silent short transfer、错误后继续 I/O、watchdog latency
 不可接受，或出现未授权写入。

## Gate D：Shutdown and close decision（Cancelled）

**状态：** Cancelled / Not Started

**原前置（未满足）：** Gate C通过。

**原计划交付（均未执行）：**

- shutdown 明确 quiesce、engine stop 和 cache durability；
- runtime read timeout 不再依赖 panic，或 RFC 明确接受并给出用户可见 fail-stop 边界；
- 更新register/current limitations与执行证据；
- 原计划在全部接受边界满足后再决定Accepted/Closed。该状态迁移已随Draft终止而永久取消，不能从本页执行；
  未满足项继续保持Open/Not Run并只由register拥有。

## 原计划不应纳入的事项（Historical）

- 不引入 IRQ worker、NCQ、multi-port scheduling、ATAPI、hotplug、partition scanner 或通用 storage
  queue。
- 不通过放宽 DMA/MMIO/capacity checks 或静默吞错来通过 gate。
- 不恢复 `driver/block/ahci` 旧路径；结构移动已经是当前 owner boundary。
