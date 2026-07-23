# AHCI Controller 不变量

## Owner 与生命周期

- `AhciController.inner` 是 controller transaction 的唯一 mutable owner；`AhciPort` 不得在
  `AtaDisk`、platform state 或 block registry 中复制 readiness、DMA 地址或 MMIO state。
- `AtaDisk` 只保存 immutable identity snapshot、devnum 和 `Arc<AhciController>` capability；identity
  字段是诊断快照，不反向驱动 port 状态机。
- `Probing -> Ready -> Recovering -> Offline` 只能由持有 `AhciPort` 的 owner 转换。只有 Ready
  接受外部 read/write；Offline 不得重新发 command。
- 一旦 port engine 或 FIS receive 可能访问 DMA metadata，`AhciPortDma` 和其 mapping 必须保持
  存活。任何 probe/error/shutdown 失败出口都必须先停止 engine，再释放 DMA/MMIO owner。

## MMIO 与 DMA

- 所有 AHCI register access 必须通过 `AhciRegs` 的 bounds-checked volatile view；production path
  不使用板载固定地址。
- HBA version、PI cardinality、port window 和 baseline host window 在首次对应 register access
  前验证；不满足时返回 probe error，不触发 `ptr_at` assertion。
- metadata page、command table、received-FIS area 和 bounce buffer 的物理范围必须完全位于
  effective DMA mask；command-list、received-FIS、command-table alignment 由类型布局和 runtime
  assertions 同时保护。
- 一个 command 只发布 slot zero、一个 PRD；PRD byte count 是 `data_len - 1`，不得接受 zero-length
  data command 的伪 PRD。
- `sync_for_device` 在发布 command issue 前完成，`sync_for_cpu` 在读取 completion metadata 或
  bounce payload 前完成；read 只有完整 transferred-byte count 匹配时才 copy out。

## ATA 与 block 边界

- 只有 capability 中同时包含 DMA/LBA、command set 中同时包含 LBA48/FLUSH CACHE EXT、logical
  sector size 为 512 bytes 且 capacity 非零的 IDENTIFY response 才能发布 block device。
- capacity 必须满足 `0 < sectors <= 2^48` 且可转换为 host `usize`；任何超界/溢出 response 必须
  返回 probe error，不能让 FIS builder 的 assertion 处理外部设备输入。
- BlockDev read/write length 必须非零、512-byte 对齐且 checked range 不超过 identity capacity。
  拆 chunk 后 LBA 和 sector count 必须在 checked conversion 内；后续 chunk 不得在前一 chunk 失败
  后继续发送。
- current write contract 只承诺 command completion 和 transfer-count correctness；在 flush command
  与 shutdown policy 明确前，不把完成写入宣称为 durable media commit。

## 并发、错误与诊断

- 当前同步实现只允许一个 active command；`SpinLock<AhciPort>` 的锁序和 IRQ-off 范围不得被
  异步 completion、nested callback 或 second request 绕过。
- 每次 command 前清除已观察的 host/port/SATA latched status；completion 后先采集状态和 transferred
  count，再清除 W1C causes，避免把旧 cause 解释为新成功。
- 任何 interrupt/task-file/link/short-transfer error 都必须先记录当前 phase、LBA（若有）、error
  分类和寄存器 snapshot，再进入 recovery 或 Offline；不可静默把 error 当作 success。
- recovery 失败、link lost 或 port 不再 present 时，readiness 必须保持 Offline；不得在未重建 command
  engine 前重新标记 Ready。
- slow-read 的 command identity 只用于诊断日志；read timeout panic 是临时 fail-stop bridge，不能
  成为长期用户可触发的正常控制流。

## 禁止退化

- 不得为了通过 probe 而放宽 MMIO/DMA bounds、capacity checks、interrupt error mask 或 link checks。
- 不得在 block registry 中再维护一份 AHCI port readiness、capacity 或 DMA address 真相源。
- 不得在当前 RFC 内增加 hotplug、NCQ、ATAPI、multi-port、IRQ queue、partition scan 或 cache flush
  的半实现；这些能力需要新的 owner/lifecycle review 和验证 gate。

