# VFS Direct User I/O Tracking Issues

**状态：** No active issues
**最后更新：** 2026-06-29
**父 RFC：** [RFC-20260629-vfs-direct-user-io](./index.md)
**事务日志：** None

本文只跟踪当前仍影响实现顺序、review gate、停止边界或验收判断的问题。修复后必须把结论折回 `index.md`、`invariants.md` 或 `implementation.md`，本页只保留问题状态和 neutralize 依据。

## Apollyon

- None.

## Keter

- None.

## Euclid

- None.

## Safe

- None.

## Neutralized

### EUCLID-003：register / current-limitations 对齐项偏窄

**原问题：** 草案只要求检查 `pwritev2` flags、`O_DIRECT` 和 file-backed mmap limitation，但本 RFC 也容易和 splice / vmsplice copy-backed 路径、ROFS direct write / mmap、open status flags 等限制混淆。

**Neutralized：** [迁移实施计划](./implementation.md) 阶段 3 已扩大 register / current-limitations 对齐清单，要求至少覆盖 `pwritev2(flags != 0)` / `RWF_*`、完整 Linux `O_DIRECT`、file-backed mmap fault、truncate / mmap coherency、ROFS direct write / writable mmap、splice / vmsplice / sendfile copy-backed path，以及 opened-description status flags / `FileIoCtx` 已有边界。该收口只做文字归属检查，不把本 RFC 误写成关闭这些独立 limitation。

### KETER-005：sequential `File.pos` 锁语义没有闭合

**原问题：** 主文档说第一阶段不改变顺序 I/O 的 `File.pos` 锁和 offset interleaving，但当前实现是在 `File::read_with_ctx()` 持 `File.pos` 锁读入 kernel buffer 后，再由 syscall helper copyout 到用户态。direct-user hook 如果在 `File` wrapper 持锁期间推进 user-buffer cursor，就会把用户页 fault、uspace mutex、分配和潜在 file-backed fault 放进 `File.pos` 锁内。这是新的锁 / 生命周期边界，不是现状保持。

**Neutralized：** 本 RFC 已接受窄 `File.pos -> user-buffer/UserSpace` 锁序作为第一阶段 sequential direct-user I/O contract，用于保持 shared opened-description offset interleaving；同时明确它不是 backend / inode / page-cache / VMO 的全局锁序。[RFC 主文档](./index.md) 的非目标、`File / FileDesc / syscall ownership`、接受边界、备选方案和风险章节已记录该取舍，并把 reserve-offset / commit-offset 留给未来单独 review。[不变量需求](./invariants.md) 已固定唯一允许的 `File.pos -> user-buffer/UserSpace` 嵌套，禁止 `UserSpace -> File.pos` 反向路径和 backend 锁内 user copy。[迁移实施计划](./implementation.md) 阶段 1C / 2、可观测性清单、停止边界和 Gate P1 / P2 已加入 source audit、失败信号和验证 floor。

### KETER-008：RFC lifecycle 倒置，公共提升被放到实现收口阶段

**原问题：** 早期草案把“公开提升预检”和 transaction devlog 创建放在实现收口阶段。按照 RFC 工作流，文档层确认后应先提升为公共 RFC；进入实现时再创建 transaction devlog，不能等实现完成后才让公共 RFC 成为 canonical source。

**Neutralized：** 本 RFC 已提升为公共 RFC，早期工作稿不再是 canonical source；进入实现前必须创建 transaction devlog 并建立双向链接。[迁移实施计划](./implementation.md) 的迁移原则和阶段 0 已把公共 RFC 导航确认和 transaction devlog 建立列为代码实现前置条件；阶段 3 已收窄为实现收口与 register 对齐。

### EUCLID-004：Phase 1A source-audit 命令路径拼接错误

**原问题：** 阶段 1A 的定向 source audit 命令把 `anemone-kernel/src/fs` 和 `anemone-kernel/src/task` 拼成了 `anemone-kernel/src/fs/anemone-kernel/src/task`，会漏查 task 子树。

**Neutralized：** [迁移实施计划](./implementation.md) 阶段 1A 的 source audit 命令已改为显式搜索 `anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/syscall`。

### KETER-006：hook 返回值和 user-buffer progress 形成双真相源

**原问题：** 早期草案让 `UserReadIter` / `UserWriteIter` 暴露 `copied()`，同时 `read_user_at` / `write_user_at` hook 又返回 `usize`。草案没有定义 wrapper 如何校验或合并二者。若 hook 已向用户 copy N 字节却返回 M，offset 推进、syscall result、fanotify notification 和用户实际可见字节会分裂；write path 还会进一步混淆 user-copy progress 和 file-visible progress。

**Neutralized：** [RFC 主文档](./index.md) 已将 cursor 形状改为 `fs/uio.rs` 中的 `UserBufferSink` / `UserBufferSource`。[不变量需求](./invariants.md) 明确 read path 的成功字节数只由 sink mark delta 派生，`read_user_at` 返回 `Result<(), SysError>`；write path 的成功字节数由 `write_user_at` 返回的 file-visible committed bytes 决定，并以 `committed <= source.bytes_since(mark)` 和 `keep_prefix_from()` 约束 source progress。[迁移实施计划](./implementation.md) 阶段 1B / 2 已把该 API 形状和一致性断言列为交付与审计项。

### KETER-007：single-buffer `EFAULT` / partial ABI 仍由 backend 选择

**原问题：** 草案禁止 ordinary vectored I/O whole-vector prevalidation，但允许 single-buffer read/write 既可以按页 / chunk 推进，也可以在产生可见 I/O 前预验证整段。这会让 cross-page bad buffer 在 ramfs、ext4 和后续 backend 间出现不同 syscall result、offset 推进和 notification 行为。

**Neutralized：** [不变量需求](./invariants.md) 已固定 single-buffer 与 vectored I/O 使用同一 page / chunk progress 规则：第一段已经产生可见进展后，后续 fault 返回已完成字节数；只有 `N == 0` 时返回 `EFAULT`。[迁移实施计划](./implementation.md) 阶段 1C / 2 保留 bad first iovec、bad later iovec 和 cross-page user buffer fault 验证，避免 backend-local policy 重新出现。

### KETER-001：`FileOps` direct-user hook 暴露 raw user memory capability

**原问题：** 早期草案把 `UserSpaceHandle` 和 raw `VirtAddr` segments 直接传给 `FileOps::{read_user_at,write_user_at}`。这会让 ramfs/ext4 各自实现用户地址验证、EFAULT 映射、segment progress 和锁序，实际把 user memory ABI 从 `fs/api/read_write` / user access 边界扩散到 backend。

**Neutralized：** [RFC 主文档](./index.md) 将 hook 签名改为 `&mut UserBufferSink` / `&mut UserBufferSource` + `FileIoCtx`，并明确 backend 不接收 `UserSpaceHandle` 或 raw segments。[不变量需求](./invariants.md) 把 user-buffer cursor 定义为用户地址访问能力的唯一 owner，并在“禁止退化项”中禁止 ordinary backend 直接接收 raw user memory capability。[迁移实施计划](./implementation.md) 阶段 1A/1B 把 `fs/uio.rs` user-buffer skeleton 和 hook signature 作为 gate。

### KETER-002：hook 存在时的 fallback 没有类型表达

**原问题：** 早期草案允许“hook 不存在或后端明确返回 fallback”时走 kernel-buffer path，但 hook 返回类型是 `Result<usize, SysError>`。如果用 `SysError::NotSupported` 或其它 errno 表达 fallback，会混淆真实用户可见错误和 VFS dispatch 策略。

**Neutralized：** [RFC 主文档](./index.md) 明确 fallback 只由 `Option` 表达：`None` 表示没有 direct-user 能力，`Some(hook)` 表示 hook 对该路径负责到底，错误就是用户可见真实错误。[不变量需求](./invariants.md) 把 “`SysError` 不得表达 fallback” 列入闭合条件和禁止退化项。[迁移实施计划](./implementation.md) 阶段 1B 要求搜索 `NotSupported` 相关 fallback 分支，确认 direct-user dispatch 没有 errno fallback。

### KETER-003：whole-vector prevalidation 会破坏 vectored I/O partial 语义

**原问题：** 早期草案允许第一阶段 hook “先验证整段，失败时不产生任何可见 I/O”。这对单 buffer 可能可接受，但对 ordinary `readv` / `writev` 会改变现有逐 segment progress：第一段已成功、第二段 EFAULT 时，应返回第一段成功字节数，而不是整体 EFAULT。

**Neutralized：** [RFC 主文档](./index.md) 将 ordinary vectored I/O 交给 user-buffer 线性 progress，拒绝 whole-vector prevalidation；exact prevalidation 只允许 fanotify metadata record 这类 transaction copyout。[不变量需求](./invariants.md) 在 “Partial 与 fault 规则” 中固定 `N > 0` 优先返回，并禁止 ordinary vectored I/O whole-vector prevalidation。[迁移实施计划](./implementation.md) 阶段 1C / 2 都要求 bad later iovec 验证。

### KETER-004：fanotify 裸 `UserSpaceHandle` copyout 与新边界不一致

**原问题：** 当前 fanotify transaction path 自己接收 raw segments 并裸调 `UserSpaceHandle` / `UserWriteSlice` 完成 metadata copyout。若 ordinary direct-user I/O 引入 user-buffer cursor 但 fanotify 保持旧形状，user memory ABI 仍存在一个特殊旁路，并且后续 helper 容易重复。

**Neutralized：** [RFC 主文档](./index.md) 将 `FileDescOps::read_user` 收窄为 `read_user_transaction`，并规定 transaction ctx 使用 `UserBufferSink` 或 restricted transaction adapter。[不变量需求](./invariants.md) 保留 fanotify transaction 所有权，同时要求 metadata copyout 通过 exact transaction helper。[迁移实施计划](./implementation.md) 阶段 1A 把 fanotify adapter 作为 direct-user `FileOps` hook 前置 gate。

### EUCLID-001：read/write API 与实现阶段过宽

**原问题：** 早期草案一次覆盖 API skeleton、read path 和 write path，容易让 write direct-user 在 read 的 fault / partial / notification 证据闭合前自然落入实现。

**Neutralized：** [迁移实施计划](./implementation.md) 将迁移拆成阶段 1A `fs/uio.rs` user-buffer skeleton + fanotify adapter、阶段 1B hook skeleton、阶段 1C read direct-user vertical slice、阶段 2 write direct-user path。阶段 2 的前置条件要求 read path 验证闭合且没有 Keter/Apollyon 阻塞项。[不变量需求](./invariants.md) 把 write path 不得提前落地列为禁止退化项。

### EUCLID-002：direct userspace copy 容易和 Linux `O_DIRECT` 混读

**原问题：** “direct user I/O” 名称可能被误解为实现了 Linux `O_DIRECT` direct I/O。当前 Anemone `O_DIRECT` 仍只是 status snapshot / compatibility 边界的一部分，不代表 alignment、page pin、bypass page cache 或 direct block I/O。

**Neutralized：** [RFC 主文档](./index.md) 在摘要、非目标、接受边界和风险中明确本 RFC 不是完整 Linux `O_DIRECT`。新类型使用 `UserBufferSink` / `UserBufferSource`，避免使用 `DirectIo` 这类会混淆 `FileOpStatusFlags::DIRECT` 的命名。[迁移实施计划](./implementation.md) 旁路审计要求 `O_DIRECT` 仍不是本 RFC direct-user copy 的完成信号。

### Neutralized：`FileDescOps::read_user` 不扩成 ordinary filesystem fast path

**结论：** 早期工作稿和本 RFC 都保留该边界。fanotify read 是 opened-description transaction，包含 event consumption、fd reservation、metadata exact copyout、commit / rollback 和 notification suppression；ordinary filesystem read/write direct-user path 放在 `FileOps` optional positioned hook 上，不复用 `FileDescOps` transaction hook。

**Repair location:** [RFC 主文档](./index.md) 的背景、方案和 fanotify adapter 章节；[不变量需求](./invariants.md) 的 fanotify 状态所有权；[迁移实施计划](./implementation.md) 阶段 1A。

### Neutralized：status flag 所有权仍归 opened file description

**结论：** Direct-user I/O 不引入第二套 status truth source。`FileDesc` 从 persistent `FileStatusFlags` 构造 `FileIoCtx`，`FileOps` 只观察短生命周期 snapshot；user-buffer cursor 不携带或缓存 status flags。

**Repair location:** [RFC 主文档](./index.md) 的背景和 `File / FileDesc / syscall ownership`；[不变量需求](./invariants.md) 的 fd / opened file description 与身份模型章节。
