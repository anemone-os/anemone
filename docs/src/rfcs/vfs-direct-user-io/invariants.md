# VFS Direct User I/O 不变量需求

**状态：** Canonical
**最后更新：** 2026-06-29
**父 RFC：** [RFC-20260629-vfs-direct-user-io](./index.md)

## 闭合条件

- 用户地址、iovec progress、copy 和 EFAULT 映射必须由 `fs/uio.rs` 中的 VFS-owned user-buffer cursor 统一表达，不能散落到每个 `FileOps` backend。
- `FileOps` direct-user hook 的存在性是 fallback 的唯一信号；hook 返回的 `SysError` 必须是用户可见真实错误。
- ordinary vectored I/O 必须保留已有 partial-progress 语义：前面 segment 已成功后，后续 segment fault 返回已完成字节数。
- fanotify read transaction 继续归 opened file description hook 所有，但 metadata copyout 必须通过同一 user-buffer adapter 收口 raw user access。
- opened file description 继续是 persistent status flags 的单一真相源，`FileOps` 只能观察 `FileIoCtx` snapshot。
- sequential direct-user I/O 第一阶段保持现有 `File.pos` 线性化点，并接受窄 `File.pos -> user-buffer/UserSpace` 锁序来保持 shared opened-description offset interleaving。

## 非目标

- 不证明完整 Linux direct I/O、page pin、zero-copy 或 bypass page cache。
- 不收口 `RWF_*` per-call flags。
- 不改变 file-backed mmap 当前 register 中记录的 fault / coherency limitation。
- 不把 fanotify control fd、pipe、eventfd、timerfd、char dev、block dev 或 procfs snapshot 自动纳入 ordinary direct-user fast path。

## 状态所有权

### `fs/api/read_write`

read-write syscall helper 拥有用户 I/O 请求的 syscall-facing 状态：

- syscall 参数解析；
- `MAX_RW_COUNT` clamp；
- iovec 导入和总长度校验；
- `UserBufferSink` / `UserBufferSource` 构造；
- ordinary direct-user path 与 kernel-buffer fallback 的调度；
- vectored I/O 总 progress 聚合；
- fanotify notification 提交；
- `pwritev2(flags != 0)` 的 unsupported gate。

read-write helper 可以看到 `UserSpaceHandle`，但不能把它直接下放给 ordinary `FileOps` backend。

### user-buffer cursor

`UserBufferSink` / `UserBufferSource` 定义在 `fs/uio.rs`，拥有用户地址访问能力和 progress 状态：

- raw user segments；
- 当前 segment index / offset；
- 已成功 copy 的字节数；
- 用户页 fault 注入和 `BadAddress` 映射；
- copy helper 的短 copy / exact copy 语义。

cursor 是一次 syscall / transaction 内的线性能力。它不得被 clone 成多个行为 owner，不得保存到 backend state，不得在 syscall 返回后继续存在。

`UserBufferSink` 表示 file -> user，用于 read family。read direct-user path 的用户可见成功字节数只由 VFS wrapper 采样 sink mark delta 派生，`read_user_at` hook 不返回 `usize`。

`UserBufferSource` 表示 user -> file，用于 write family。source consumed bytes 只说明用户数据已经被 copy 到 backend 临时目标；write direct-user path 的用户可见成功字节数由 `write_user_at` 返回的 file-visible committed bytes 决定，并且必须满足 `committed <= source.bytes_since(mark)`。

### fd / opened file description

`FileDesc` 拥有 fd-local Linux-visible policy：

- read/write access；
- path-only gate；
- status flags snapshot，包括 `O_APPEND`、`O_NONBLOCK`、`O_DIRECT`、`O_SYNC`、`O_DSYNC`、`O_NOATIME`；
- `O_APPEND` 与 positioned write 的决策；
- opened-description transaction hook 和 notification suppression。

`FileDesc` 构造 `FileIoCtx` 后才能进入 `File` / `FileOps` data I/O。backend 不得反向读取 `FileDesc` 或 fd table。

### VFS opened file / `File`

`File` 拥有 opened file object 共享状态和 VFS-wide gate：

- `File.pos` 及其锁；
- zero-length I/O fast path；
- regular content 的 readonly mount gate；
- successful write 后 inode metadata update；
- 顺序 direct-user I/O 对 `File.pos` 的推进。

顺序 direct-user read/write 第一阶段维持 `File.pos` 作为 shared opened-description cursor 的线性化点：`File` wrapper 在持有 `File.pos` guard 的范围内调用 direct-user hook 并推进 user-buffer cursor，只按 read copied bytes 或 write committed bytes 推进 offset。

这条规则引入一个窄锁序：`File.pos -> user-buffer copy -> UserSpace`。该锁序只服务 sequential data I/O 的 cursor 互斥，不把 `File.pos` 提升为 backend I/O、page cache、inode metadata 或 VMO fault 的全局串行化锁。任何缩短持锁范围、reserve-offset / commit-offset 协议、锁外异步 copy 或 shared opened-description offset interleaving 变化，都必须回到 RFC review。

### backend / `FileOps`

backend 只拥有文件内容和自身存储结构：

- EOF、file size、hole / zero-fill 策略；
- page/frame lookup、allocation、cache 和 dirty state；
- filesystem-specific error；
- 是否安装 direct-user hook；
- write path 的 size / dirty / metadata commit。

backend 不拥有用户地址 ABI，不得保存或暴露 `UserSpaceHandle`、raw user segments、fd number、`FileDesc` 或 task file table。

### fanotify

fanotify group fd read 仍是 opened-description transaction：

- group queue pop / wait；
- event object opening；
- fd reservation / commit / rollback；
- metadata record construction；
- notification suppression；
- control fd access notification policy。

fanotify copyout 使用 `UserBufferSink` 的 exact transaction helper。copyout helper 可以为完整 metadata record 预验证用户目标区间；这条规则不能推广到 ordinary vectored I/O。

## 身份与能力模型

- `FileIoCtx` 是 opened-description status flags 的短生命周期 snapshot，不是新的 truth source。
- `UserBufferSink` / `UserBufferSource` 是 user memory access capability。它们的 identity 只在本次 syscall / transaction 内有效。
- `read_user_at` / `write_user_at` hook 存在表示 backend 声明该 file type 支持 ordinary direct userspace copy。不存在表示 fallback；存在后不能再把 fallback 当成运行时 outcome。
- `read_user_transaction` 是 opened-description transaction capability，不是 file content direct-user capability。
- fanotify exact copy helper 是 transaction-specific adapter，不得被 ordinary file I/O 用来改变 partial semantics。

## 线性化点

- Ordinary read syscall 的用户可见成功字节数由 `UserBufferSink` mark delta 决定；`read_user_at` hook 不返回单独字节数，不能制造第二套 truth source。
- Ordinary write syscall 的用户可见成功字节数由 backend 返回的 file-visible committed bytes 决定；`UserBufferSource` consumed bytes 只能作为一致性上限和 cursor 回退依据。
- 顺序 direct-user read/write 的 `File.pos` 推进发生在 `File` wrapper 中，只按 VFS 派生出的 read copied bytes、write committed bytes 或 fallback 返回的成功字节推进。
- Positioned direct-user read/write 不读取、不更新普通 `File.pos`。
- `FAN_ACCESS` / `FAN_MODIFY` 只在最终成功字节数大于 0 时提交一次。
- fanotify path-event fd commit 发生在完整 metadata copyout 成功之后；copyout 失败只能 rollback reservation，不得发布用户未收到的 fd。
- write direct-user path 如果已把内容写入 file-visible storage，后续错误不得覆盖已有 progress；要么返回成功字节数，要么在产生可见 I/O 前失败。

## Partial 与 fault 规则

- `N > 0` 字节已经成功 copy / 写入 / 读出时，后续 `EFAULT`、overflow 或 backend error 不能把 syscall result 改成错误。
- `N == 0` 时返回原始错误。
- user-buffer 普通 copy helper 可以短 copy；read wrapper 用 sink delta 推进 file offset / syscall result，write hook 只能用已提交字节推进 file offset / syscall result。
- ordinary `readv` / `writev` 不允许 whole-vector prevalidation。如果第一段成功、第二段 fault，结果必须是第一段成功字节数。
- 单 buffer read/write 采用与 vectored I/O 一致的 page / chunk progress 规则，不能把 cross-page bad buffer 留给 backend-local policy。第一段已经产生可见进展后，后续 fault 返回已完成字节数；只有 `N == 0` 时返回 `EFAULT`。
- fanotify exact metadata record 是特殊 transaction：允许先验证完整 record 用户目标区间，失败时不写半条 record。

## 锁序与生命周期规则

- 顺序 direct-user wrapper 可以在持有 `File.pos` guard 时访问 user-buffer cursor；这是唯一接受的 `File.pos -> user-buffer/UserSpace` 嵌套。
- 不允许持有 `UserSpace` guard 后反向调用会获取 `File.pos` 的 sequential `File::read`、`File::write`、`File::seek`、`File::append` 或等价 opened-description cursor API。
- 如果 user-space fault 后续需要 file-backed backing 或 page-cache 数据，必须走不读取普通 `File.pos` 的 positioned / cache-owned 路径；不得通过 opened-description sequential cursor 回调 VFS。
- backend 不能在持有可能睡眠、可能重入或会扩大锁序的 inode / page-cache / backend 全局锁时访问 user-buffer cursor。
- backend 可以在短锁段中定位并 clone / pin 稳定 page/frame handle 或短生命周期数据片段，然后释放后端结构锁，再调用 user-buffer copy。
- write path 在 user copy 完成后用明确短锁段提交 dirty / size / metadata。
- 不允许在 IRQ-off、preemption-disabled 或 spinlock-held context 触发用户 copy 或 page fault。
- `UserBufferSink` / `UserBufferSource` 不能逃逸出 syscall / transaction；backend 不得保存引用、raw pointer 或内部 segment。
- cleanup / rollback 路径必须先撤销已发布能力，再用断言暴露 bug；不能因为 copyout failure 泄漏 fanotify fd reservation。

## 禁止退化项

- 不得让 ordinary `FileOps` hook 直接接收 `UserSpaceHandle`、raw `VirtAddr` segment 或 `OpenedFileReadUserSegment`。
- 不得用 `SysError` 表达 direct-user fallback。
- 不得让 direct-user cursor 成为可 clone 的多 owner progress 状态。
- 不得把 `File.pos -> user-buffer/UserSpace` 推广成 backend / inode / page-cache / VMO 的全局锁序。
- 不得引入 `UserSpace -> File.pos` 的反向锁序。
- 不得在 backend 私有状态中缓存 fd status flags、user segment 或 task identity。
- 不得把 fanotify `read_user_transaction` 改造成 ordinary file content I/O。
- 不得把 whole-vector prevalidation 用于 ordinary vectored I/O。
- 不得把 `O_DIRECT` 语义混同为本 RFC 的 direct userspace copy。
- 不得把 read/write Phase 2 的 write direct-user path 在 read gate 未闭合前自然落入实现。

## 完成标准

- `tracking-issues.md` 中 Keter / Euclid 项都能指向本文或 [迁移实施计划](./implementation.md) 的 canonical 修复文本。
- `fs/uio.rs` 提供唯一 user-buffer cursor 类型，`fs/api/read_write` 有唯一构造和 progress 聚合入口。
- ordinary `FileOps` direct-user hook 不接收 raw user memory capability。
- fanotify read transaction 不再裸调 `UserSpaceHandle` 或遍历 raw user segments。
- ramfs/ext4 read direct-user hook 的锁序、partial 和 EFAULT 行为有定向验证。
- write direct-user path 只有在 read gate 验证后才进入实现。
