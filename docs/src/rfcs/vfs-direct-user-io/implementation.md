# VFS Direct User I/O 迁移实施计划

**状态：** Completed
**最后更新：** 2026-06-29
**父 RFC：** [RFC-20260629-vfs-direct-user-io](./index.md)
**不变量：** [不变量需求](./invariants.md)

## 迁移原则

- 本目录是 `vfs-direct-user-io` 的 canonical plan；实现开始前必须创建 transaction devlog 并与 RFC 建立双向链接。
- 先收口 user memory capability，再接 ordinary `FileOps` direct-user fast path。
- read 与 write 的 API 和不变量一次定义，但代码实现分阶段：先 read，后 write。
- `Option` 是 direct-user fallback 的唯一表达；hook 存在后不得返回“请 fallback”。
- `UserBufferSink` / `UserBufferSource` 是唯一普通 user-buffer cursor，代码放在 `fs/uio.rs`。backend 不能直接接触 `UserSpaceHandle` 或 raw user segments。
- fanotify 裸 user copyout 和 ordinary file direct-user I/O 使用同一 user-buffer 基础设施，但 fanotify 仍走 opened-description transaction hook。
- 每个阶段必须保持可构建，且有搜索审计和验证 floor。
- 若实现反馈暴露 ABI 边界、partial 规则、锁序或 `File.pos` 语义需要变化，停止当前 gate，回写 RFC canonical 文本后再继续。

## 阶段 0：实施前审计

前置条件：

- 本公共 RFC 已就位，后续实现不再依赖非公共工作稿。
- [Tracking Issues](./tracking-issues.md) 中仍会改变 accepted contract、状态所有权、ABI 边界、阶段顺序或验收判断的 active Keter/Apollyon 已 neutralize，或已明确转成某个实现 gate 的停止条件。

交付：

- 确认公共 RFC 导航、`docs/src/rfcs.md`、`docs/src/SUMMARY.md` 和 RFC 内相对链接已同步。
- 在进入任何代码实现前创建 transaction devlog，并在公共 RFC 与 transaction 之间建立双向链接。
- 用搜索确认当前 read/write、fanotify、FileOps 和 user access 的写集：

```sh
rg -n "read_user|OpenedFileReadUser|UserSpaceHandle|UserReadSlice|UserWriteSlice|FileIoCtx|FileOps \\{|read_at|write_at|FAN_ACCESS|FAN_MODIFY" anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/syscall
```

- 记录当前 fanotify read transaction 中 event pop、fd reservation、metadata copyout 和 commit / rollback 的调用顺序。
- 记录 ramfs/ext4 regular file 当前 page/frame copy_in / copy_out 的锁序。

审计：

- 确认 `FileDescOps::read_user` 当前只用于 opened-description transaction，而不是普通 filesystem read。
- 确认 `pwritev2(flags != 0)` 当前 limitation 仍存在，不纳入本 RFC 实现。

验证：

- docs-only 阶段不运行 QEMU / LTP。
- 公共导航变化后运行 `git diff --check` 和 `mdbook build docs`。
- 开始代码实现前至少运行 `just build`，确认不是从已坏基线开始。

退出条件：

- 写集和现有裸 user access 点清楚。
- 公共 RFC 和 transaction devlog 已就位，后续实现只引用公共 RFC 作为 canonical source。
- 没有发现需要先回到 RFC 的额外 shared-contract 问题。

## 阶段 1A：`fs/uio.rs` user-buffer skeleton 与 fanotify adapter

前置条件：

- 阶段 0 审计完成。

交付：

- 在 `anemone-kernel/src/fs/uio.rs` 新增 `UserBufferSink` / `UserBufferSource` 或等价类型。
- cursor 从 `UserSpaceHandle` 和 checked iovec / single buffer 构造，但不向 backend 暴露 raw segments。
- cursor 提供 ordinary copy/progress 方法：
  - `remaining()`；
  - `UserBufferSink` 的 file -> user copy helper 和 mark / delta 采样；
  - `UserBufferSource` 的 user -> file copy helper、mark 和 `keep_prefix_from()`。
- cursor 提供 fanotify 专用 exact transaction helper，支持完整 metadata record 预验证和失败无半写。
- 将 `FileDescOps::read_user` 重命名为 `read_user_transaction`。
- 将 transaction ctx 的 raw `segments` / `uspace` 替换为 `UserBufferSink` 或 restricted transaction adapter。
- fanotify read transaction 改用 exact helper copyout，保留 event pop、fd reservation、commit / rollback 和 `notify_read_user_access: false` 语义。

审计：

- 搜索 `OpenedFileReadUserSegment`，确认它已删除或只作为临时 compatibility 名称存在，并有删除点。
- 搜索 `fanotify.*uspace.lock|UserWriteSlice`，确认 fanotify metadata copyout 不再裸调 user access。
- 搜索 `read_user:` initializer，确认字段名和语义已收窄为 transaction。

反馈假设：

- `UserBufferSink` 能同时表达 ordinary partial copy 和 fanotify exact transaction copy，不需要让 fanotify 保留 raw segments。

失败信号：

- fanotify copyout 需要访问 cursor 内部 raw segments 才能实现 fd rollback；
- exact helper 无法保证坏第二个 iovec 不半写 metadata；
- transaction ctx 被迫携带完整 `FileDesc` 或 task。

回写路径：

- 若 exact transaction helper 不足，更新 [不变量需求](./invariants.md) 的 fanotify adapter contract 和本阶段交付。

模块边界预检：

- 如果 `fs/api/read_write/mod.rs` 已经同时混合 syscall ABI、iovec import、copy helper、notification 和 direct-user dispatch，允许在同一 owner 内做行为保持的目录化拆分，例如 `read_write/notify.rs`。user-buffer cursor 本身固定放在 `fs/uio.rs`，不再放到 `read_write/user_iter.rs`。
- split-only checkpoint 不得改变 syscall ABI、`FileDesc` public surface 或 fanotify transaction 语义。

write set：

- 默认允许：`anemone-kernel/src/fs/uio.rs`、`anemone-kernel/src/fs/mod.rs`、`anemone-kernel/src/fs/api/read_write/*`、`anemone-kernel/src/task/files.rs`、`anemone-kernel/src/fs/fanotify/file.rs`、必要的 module re-export。
- 不应触碰：ramfs/ext4 direct hook、write direct-user path、public docs。

可观测性：

- 对 fanotify transaction helper 增加轻量 `assert!`，确认 exact copy 的字节数等于 metadata record 长度。
- 保留或增加 fanotify fd reservation rollback 的现有诊断上下文。

验证：

- `just fmt kernel`
- `just build`
- 定向 source audit：

```sh
rg -n "UserSpaceHandle|OpenedFileReadUserSegment|read_user_transaction|UserBufferSink|UserBufferSource|UserRecordSink|UserWriteSlice" anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/syscall
```

退出条件：

- fanotify transaction 不再裸访问 user segments。
- ordinary read/write 行为仍全部 fallback 到旧 kernel-buffer path。
- 没有 direct-user `FileOps` hook 被安装。

## 阶段 1B：FileOps optional hook skeleton 与 None-only fallback

前置条件：

- 阶段 1A 通过构建和 source audit。

交付：

- 在 `FileOps` 增加 optional `read_user_at` / `write_user_at` 字段。
- hook 签名接收 `&mut UserBufferSink` / `&mut UserBufferSource` 和 `FileIoCtx`，不接收 `UserSpaceHandle` 或 raw segments。
- `read_user_at` 返回 `Result<(), SysError>`；VFS wrapper 采样 sink mark delta 派生 read 成功字节数。
- `write_user_at` 返回 `Result<usize, SysError>`；该 `usize` 只表示 file-visible committed bytes，并用 `assert!(written <= src.bytes_since(mark))` 约束。
- 所有现有 initializer 默认 `None`。
- `File` / `FileDesc` 提供 read direct-user wrapper 的 skeleton，但 hook 为 `None` 时行为保持，仍走 kernel-buffer fallback。
- 文档化代码注释：`None` 是 fallback 的唯一表达，hook 内错误不能被当作 fallback。
- 暂不接 write direct path；`write_user_at` 字段只作为 API shape 保留。

审计：

- 搜索所有 `FileOps {` initializer，确认 `read_user_at` / `write_user_at` 显式为 `None` 或缺省构造路径已经统一可审计。
- 搜索 direct-user hook 签名，确认没有 `UserSpaceHandle`、`VirtAddr` segment 或 `FileDesc` 参数。
- 搜索 `NotSupported` 相关 fallback 分支，确认 direct-user dispatch 没有用 errno fallback。

反馈假设：

- `Option` 足以表达 fallback，不需要 `Outcome::Fallback`。

失败信号：

- 某 backend 必须在 hook 存在时根据运行时条件回退到 kernel-buffer path。

回写路径：

- 如果运行时 fallback 确实必要，必须回到 RFC review，选择显式 outcome 类型；不能临时用 `SysError` 表达 fallback。

模块边界预检：

- `fs/file.rs` 若因新增 hook 继续变大，可以只做同一 owner 内的局部 helper 整理；不得在本阶段拆出新的 public API facade。

write set：

- 默认允许：`anemone-kernel/src/fs/file.rs`、`anemone-kernel/src/fs/uio.rs`、所有 `FileOps` initializer、`anemone-kernel/src/task/files.rs`、`anemone-kernel/src/fs/api/read_write/*`。
- 不应触碰：ramfs/ext4 hook implementation、write direct path semantics。

验证：

- `just fmt kernel`
- `just build`
- source audit：

```sh
rg -n "read_user_at|write_user_at|UserSpaceHandle|Fallback|NotSupported" anemone-kernel/src/fs anemone-kernel/src/task
```

退出条件：

- repo 范围 static vtable 构建闭合。
- hook skeleton 存在但不改变普通 read/write 行为。
- fallback 规则在代码和 RFC 中一致。

## 阶段 1C：Read direct-user path for ramfs/ext4

前置条件：

- 阶段 1A/1B 通过。
- 用户接受 read path 作为第一批行为 gate。

交付：

- `fs/api/read_write` 在 non-transaction read path 构造 `UserBufferSink` 并尝试 direct-user read。
- 顺序 `read` / `readv` 在没有 `read_user_transaction` 时进入 ordinary direct-user wrapper。
- `pread` / `preadv` 直接进入 positioned direct-user wrapper，不进入 transaction hook。
- sequential wrapper 持 `File.pos` guard 调用 direct-user hook 并推进 user-buffer cursor，只按实际 sink delta 推进 offset。
- ramfs regular file 安装 `read_user_at`。
- ext4 regular file 安装 `read_user_at`。
- backend 在短锁段中定位 / clone stable frame 或数据片段，释放后端锁后调用 `UserBufferSink` copy。
- 其它 backend 保持 `None` fallback。
- successful bytes > 0 时只提交一次 `FAN_ACCESS`。

审计：

- 检查 ramfs/ext4 direct read hook，不得在持有 pages map write/read lock、filesystem transaction lock、spinlock 或 no-preempt context 下触发 user copy。
- 检查 sequential direct-user wrapper 的唯一新增嵌套是 `File.pos -> user-buffer/UserSpace`；不得出现持 `UserSpace` guard 后反向进入 sequential `File::read` / `write` / `seek` / `append` 的路径。
- 检查 file-backed fault / VMO backing 后续若触达文件数据，只走 positioned / cache-owned 路径，不使用 opened-description `File.pos` cursor。
- 检查 readv bad later iovec 行为：前面 segment 已完成后，后续 EFAULT 返回已完成字节数。
- 检查 single-buffer cross-page fault 行为：该 gate 不把旧 trampoline 的整段 copyout / copyin 粗粒度行为当作保持目标；若 direct-user path 暴露更细粒度 progress，以本 RFC 的 partial / fault contract 为准，并记录验证证据。
- 检查 positioned read 不修改 `File.pos`。
- 检查 transaction read path 优先级：fanotify group fd 不进入 ordinary `FileOps` direct-user read。

反馈假设：

- ramfs/ext4 当前 frame/page cache 足以在不持 backend 结构锁 copy user 的前提下实现 read direct path。

失败信号：

- ext4 page cache / lwext4 read path 只能在 filesystem lock 内提供可访问 slice；
- `UserBufferSink` 不能表达 cross-page partial；
- 实现需要 reserve-offset / commit-offset、缩短 `File.pos` 持锁范围，或出现 `UserSpace -> File.pos` 反向锁序；
- direct read 让 fanotify notification 重复或漏报。

回写路径：

- 锁序或 user-buffer 能力问题更新 [不变量需求](./invariants.md)；
- 阶段顺序或 write set 变化更新本文；
- 接受的性能或 fallback 边界进入 register / current limitations。

模块边界预检：

- 若 ramfs/ext4 file modules 已经混合 mmap, read/write, truncate, direct-user helper，可做同一 owner 内的局部 helper 拆分，但不得改变 file ops public contract。

write set：

- 默认允许：`fs/uio.rs`、`fs/api/read_write/*`、`fs/file.rs`、`task/files.rs`、`fs/ramfs/file.rs`、`fs/ext4/file.rs`、fanotify notification helper 相关窄调用。
- 不应触碰：write direct path、non-regular backend direct hooks、RWF flags。

可观测性：

- 关键 invariant 使用 `assert!`：read wrapper 采样到的 sink delta 不得超过起始 remaining；fanotify notification helper只在最终成功字节数大于 0 时触发。
- 对 backend 因能力不足不安装 hook，不打热路径日志。

验证：

- `just fmt kernel`
- `just build`
- 定向用户态 / kernel smoke：
  - regular file `read` / `readv` / `pread` / `preadv`；
  - bad first iovec -> `EFAULT` 且 offset 不推进；
  - bad later iovec -> 返回已完成字节数；
  - cross-page user buffer fault；
  - EOF / short read；
  - positioned read 不修改 file cursor；
  - fanotify `FAN_ACCESS` 只提交一次。
- 用户侧目标测例 / 性能路径验证后才能进入 Phase 2。

退出条件：

- read direct-user path 在 ramfs/ext4 regular file 上通过 targeted semantics。
- no-hook fallback 与旧 kernel-buffer path 行为等价。
- 用户确认目标测例或性能路径可进入下一阶段。

## 阶段 2：Write direct-user path for ramfs/ext4

前置条件：

- 阶段 1C read path 验证闭合。
- 没有开放 Keter/Apollyon tracking issue 阻塞 write path。

交付：

- `fs/api/read_write` 在 write path 构造 `UserBufferSource` 并尝试 direct-user write。
- `FileDesc` 继续拥有 access、path-only、status snapshot 和 `O_APPEND` 决策。
- `File` 继续拥有 readonly mount gate、sequential `File.pos`、scoped `File.pos -> user-buffer/UserSpace` 锁序和 successful write metadata update。
- ramfs/ext4 regular file 安装 `write_user_at`。
- backend 从 `UserBufferSource` copy 到 stable page/frame 后，用明确短锁段提交 dirty / size / metadata。
- 其它 backend 保持 `None` fallback。
- successful bytes > 0 时只提交一次 `FAN_MODIFY`。

审计：

- 检查 copyin、dirty、size 和 inode metadata update 是否遵守 partial rule。
- 检查 `write_user_at` 返回值只表达 file-visible committed bytes，source consumed bytes 只作为上限和 `keep_prefix_from()` 回退依据。
- 检查 `O_APPEND + pwrite/pwritev` 与现有语义一致。
- 检查 sequential write direct-user path 不引入 `UserSpace -> File.pos` 反向锁序，且 backend 不在持 filesystem / page-cache / inode 锁时推进 `UserBufferSource`。
- 检查写入已产生可见内容后不能返回错误覆盖 progress。
- 检查 readonly mount gate 不被 backend 绕过。

反馈假设：

- ramfs/ext4 write path 能按 page/chunk 复制用户数据并在 copy 后提交 dirty / size，不需要持 backend 全局锁跨 user fault。

失败信号：

- backend write commit 可能在内容已可见后失败且无法返回 partial bytes；
- user copy 必须在 filesystem transaction lock 内发生；
- 实现需要 reserve-offset / commit-offset、缩短 `File.pos` 持锁范围，或出现 `UserSpace -> File.pos` 反向锁序；
- append 与 positioned write 在 direct path 下分流不清。

回写路径：

- 若 write path 需要不同 transaction model，回到 RFC review，不允许用临时 whole-buffer copy 隐藏 partial 语义变化。

模块边界预检：

- 如果 ext4 write direct path 需要调整 page cache helper，可做 ext4 owner 内的 helper 拆分；不得扩大到完整 page cache RFC。

write set：

- 默认允许：`fs/uio.rs`、`fs/api/read_write/*`、`fs/file.rs`、`task/files.rs`、`fs/ramfs/file.rs`、`fs/ext4/file.rs`。
- 不应触碰：RWF flags、non-regular backend direct hooks、O_DIRECT direct I/O。

验证：

- `just fmt kernel`
- `just build`
- 定向用户态 / kernel smoke：
  - `write` / `writev` / `pwrite` / `pwritev`；
  - bad first iovec -> `EFAULT` 且 offset / size 不推进；
  - bad later iovec -> 返回已完成字节数；
  - cross-page fault；
  - `O_APPEND` / `pwrite`；
  - size update、dirty 标记和 metadata update；
  - `FAN_MODIFY` 只提交一次。

退出条件：

- write direct-user path 在 ramfs/ext4 regular file 上通过 targeted semantics。
- 旧 kernel-buffer fallback 仍可用于 non-hook backend。

## 阶段 3：实现收口与 register 对齐

前置条件：

- read path 或 read+write path 已完成当前实现目标。

交付：

- 更新公共 RFC 状态和 transaction devlog 收口记录；不得把已执行 checkpoint 写回非公共工作稿作为 canonical source。
- 检查 register / current-limitations 中容易与本 RFC 混读的条目是否需要调整文字；不要把本 RFC 误写成关闭这些 limitation。至少覆盖：
  - `pwritev2(flags != 0)` / `RWF_*` per-call flags；
  - `O_DIRECT` Linux direct I/O；
  - file-backed mmap fault、truncate / mmap coherency、ROFS direct write / writable mmap；
  - splice / vmsplice / sendfile 这类 copy-backed 或 in-kernel transfer path；
  - opened-description status flags / `FileIoCtx` 已有边界。
- 清理 tracking issues：仍开放的 design issue 保持 Active；已由实现证据处理的项移到 Neutralized。

验证：

- docs-only：`git diff --check`；若公共 docs 导航变化，`mdbook build docs`。
- 非公共工作稿不作为实现期 canonical source 证据。

退出条件：

- 文档层、实现事实、register / limitations 归属一致。

## 旁路审计清单

实现期间至少执行：

```sh
rg -n "UserBufferSink|UserBufferSource|UserRecordSink|read_user_transaction|read_user_at|write_user_at|UserSpaceHandle|OpenedFileReadUserSegment|FAN_ACCESS|FAN_MODIFY|pwritev2|O_DIRECT" anemone-kernel/src
```

分类要求：

- ordinary backend direct-user hook 不得出现 `UserSpaceHandle` 或 raw segment 参数；
- fanotify 可以出现 transaction hook，但不裸遍历 user segments；
- `NotSupported` 不得作为 direct-user fallback；
- `pwritev2(flags != 0)` 仍独立 fail closed；
- `O_DIRECT` 仍不是本 RFC direct-user copy 的完成信号。

## 可观测性清单

- user-buffer progress 必须能在 debug/review 中关联 syscall request、segment progress、read sink delta、write committed bytes 和 returned bytes。
- fanotify exact copy helper 的 failure path 必须能确认 fd reservation rollback。
- direct read/write helper 的 notification path 必须能确认一次成功 syscall 最多提交一次 `FAN_ACCESS` / `FAN_MODIFY`。
- backend hook 中的锁序必须能通过 source audit 看出 user copy 不在 backend 全局锁内发生。
- sequential direct-user wrapper 的锁序必须能通过 source audit 看出只有 `File.pos -> user-buffer/UserSpace`，没有 `UserSpace -> File.pos` 反向路径。

## 停止边界

以下情况必须停止当前 gate 并回到 RFC review：

- 需要让 backend 保存 `UserSpaceHandle`、user segment 或 cursor；
- 需要用 errno 表达 fallback；
- 需要 whole-vector prevalidation 才能让 ordinary read/write path 可实现；
- 需要缩短 `File.pos` 持锁范围、引入 reserve-offset / commit-offset 协议，或改变 shared opened-description offset interleaving 语义；
- 需要从 `UserSpace` fault / copy 路径反向获取 ordinary `File.pos`；
- write path 无法在可见内容和错误返回之间建立 partial-progress contract；
- 实现发现 `RWF_*` 或完整 `O_DIRECT` 不可避免地进入 direct-user ctx。

Euclid / Safe 级别的命名、目录化或 helper 拆分问题可以进入实现 gate，但必须保持 write set 和验证 floor 明确。

## Probe / Vertical Slice Gates

### Gate P1 - read direct-user vertical slice

**Hypothesis:** `UserBufferSink` + optional `read_user_at` 足以让 ramfs/ext4 regular file read 绕过 kernel buffer，同时保持 readv partial、EFAULT、offset 和 fanotify notification 语义。

**Protected Goal / Invariant:** backend 不接收 raw user memory capability；`None`-only fallback；`N > 0` progress 优先返回；fanotify transaction 不走 ordinary `FileOps` path；sequential direct-user path 只有窄 `File.pos -> user-buffer/UserSpace` 锁序。

**Minimum Write Set:** `fs/uio.rs`、`fs/api/read_write/*`、`fs/file.rs`、`task/files.rs`、`fs/ramfs/file.rs`、`fs/ext4/file.rs`、`fs/fanotify/file.rs` 中的 user-buffer adapter。

**Non-goals:** write direct-user path、RWF flags、O_DIRECT、non-regular backend hooks、zero-copy。

**Validation Floor:** `just fmt kernel`、`just build`、targeted bad-iovec / partial / positioned-read smoke；source audit 确认无 backend 锁内 user copy、无 `UserSpace -> File.pos` 反向路径；用户侧目标测例或性能路径。

**Failure Signal:** bad later iovec 返回 `EFAULT` 而非 completed bytes；positioned read 改变 `File.pos`；fanotify `FAN_ACCESS` 重复或漏报；backend 需要裸 `UserSpaceHandle`；实现需要 reserve-offset、缩短 `File.pos` 持锁范围或反向获取 `File.pos`。

**Exit Path:** 成功则进入 write gate；失败则把具体原因回写到本文或 [不变量需求](./invariants.md)，不能用 compatibility fallback 隐藏。

### Gate P2 - write direct-user vertical slice

**Hypothesis:** `UserBufferSource` + optional `write_user_at` 足以让 ramfs/ext4 regular file write 绕过 kernel buffer，同时保持 writev partial、EFAULT、offset、append、dirty / size / metadata 和 fanotify notification 语义。

**Protected Goal / Invariant:** backend 不接收 raw user memory capability；`None`-only fallback；user-copy progress 与 file-visible committed bytes 分离；`N > 0` committed progress 优先返回；readonly mount gate 和 `O_APPEND` 决策仍归 `File` / `FileDesc`；sequential direct-user path 只有窄 `File.pos -> user-buffer/UserSpace` 锁序。

**Minimum Write Set:** `fs/uio.rs`、`fs/api/read_write/*`、`fs/file.rs`、`task/files.rs`、`fs/ramfs/file.rs`、`fs/ext4/file.rs`、fanotify modify notification 的窄调用点。

**Non-goals:** RWF flags、O_DIRECT、non-regular backend hooks、zero-copy、page pin、reserve-offset / commit-offset。

**Validation Floor:** `just fmt kernel`、`just build`、targeted bad-iovec / partial / positioned-write / append / readonly smoke；source audit 确认 backend 不在 filesystem、inode、page-cache 或 spin/noirq 锁内推进 `UserBufferSource`；用户侧目标测例或性能路径。

**Failure Signal:** bad later iovec 覆盖已有 committed progress；`write_user_at` 返回 user-copy bytes 而非 file-visible committed bytes；`O_APPEND + pwrite/pwritev` 与现有语义分裂；readonly mount gate 被 backend 绕过；内容已可见后仍返回错误覆盖 progress；实现需要 reserve-offset、缩短 `File.pos` 持锁范围或反向获取 `File.pos`。

**Exit Path:** 成功则进入实现收口；失败则把具体原因回写到本文、[不变量需求](./invariants.md) 或 RFC 主文档，不能用 whole-buffer copy、errno fallback 或 silent partial rollback 隐藏。
