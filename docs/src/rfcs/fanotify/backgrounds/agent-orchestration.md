# Fanotify Agent 编排建议

本文记录 `fanotify` 进入实现阶段时的 agent 编排方式。Canonical 协议仍以
[RFC 入口](../index.md)、[不变量需求](../invariants.md)、
[迁移实施计划](../implementation.md) 和 [Tracking Issues](../tracking-issues.md)
为准；本文只说明如何按依赖关系组织 worker、reviewer 和验收顺序。

本 RFC 尚未进入实现阶段。真正开始实现前，总控 agent 必须先建立事务 devlog，并在
RFC 入口和事务日志之间建立双向链接。

## 编排原则

1. fanotify 是完整新子系统，不是已有基础上的局部迭代。拆分边界必须对应状态所有权、
   通用 fd/task 能力、registry/matching、path-fd read 提交协议和 VFS hook 合流点，
   不能按 “Stage 0 一个 agent、Stage 1 一个 agent” 机械切分。
2. `fs::fanotify` 必须是 group、registry、queue、mark record、matching 和 file ops
   private state 的 owner。syscall、VFS、task/fd 和 procfs worker 只能接触 typed
   facade，不能 downcast fanotify private state。
3. Stage 0 与 Stage 1 是同一个用户可见 gate。`fanotify_init()` 成功返回 group fd
   前，blocking/nonblock read、poll、close wakeup 和 bounded queue 必须同时闭合。
4. task/fd 层通用能力是多个后续节点的根依赖：current opened file description
   status flags、fd reservation/commit/rollback、opened-description release hook 和
   notification suppression。缺任一项时，不要让 fanotify worker 在私有状态里临时补
   一份并行真相源。
5. mark registry 可以在 group/facade 确定后独立推进；真实 VFS enqueue 不能早于
   registry key/lifecycle、queue cap 和 group dead/target dead 规则闭合。
6. path-fd read 提交协议必须先于真实 VFS hook。event fd 安装、metadata copyout 和
   rollback 没闭合时，VFS hook 只能停在 synthetic / no-user-visible 证据。
7. `FAN_CLOSE_*` 归 opened file description release source，不归 fd number close。若为
   单 fd LTP 场景保留 temporary bridge，必须在事务日志和代码注释中标成降级路径，不能
   用来通过 Stage 3 退出条件。
8. FID/name、permission event、pidfd、procfs fdinfo 和 Stage 5 backlog 只能在基础
   path-fd 通知闭合后选择性启动。任何 worker 都不得用接受 flag 但无真实可观测语义的
   方式换取短期通过。
9. review agent 放在依赖合流点：Stage 0+1 合并 gate、registry gate、path-fd commit
   gate、基础 VFS event gate、FID/name negative gate 和最终旁路审计。不要在每个小
   helper 后立即审查。
10. 写入型 worker 默认只改自己的 write set；若更合适的架构必须扩大范围，停止并向
    总控提交 write set 扩展申请，批准后再继续。

## 依赖拓扑

以下节点描述实现依赖，不等同于文档阶段：

- **D0：前置审计。** 确认当前代码仍只有 syscall number、无 fanotify subsystem；确认
  read/write/poll/FileOps、task fd table、anonymous inode、VFS open/read/write/close
  和 LTP profile 的实际落点。
- **D1：task/fd 通用能力。** 为下游提供 current status flags read context、reserved
  fd slot、commit/rollback、opened-description release callback 和通用 notification
  suppression。D1 是 D2 的 nonblock gate、D4 的 path-fd gate、D5 的 close event gate
  的共同根节点。
- **D2：fanotify owner module 与 group fd。** 建立 UAPI parser、syscall facade、
  anonymous group fd、bounded queue、poll/read/close wakeup 和 fail-closed vtable。
  D2 可以在 D1 进行时准备内部类型，但公开 `fanotify_init()` 成功必须等 D1 中 current
  status flags/read context 至少闭合。
- **D3：mark registry 与 `fanotify_mark()`。** 依赖 D2 的 group identity/facade；
  产出 registry key、MarkHandle、group-owned cleanup handles 和 add/remove/flush
  线性化规则。
- **D4：path-fd read 提交协议。** 依赖 D1 的 fd reservation/no-notify 和 D2 的 group
  queue；用 synthetic 或 queued event 先证明 metadata record 不半提交、copyout 失败
  rollback 和 event fd no-notify。
- **D5：VFS event injection。** 依赖 D2/D3/D4 和 D1 的 release hook；接入 open、
  read/read_at、write/write_at/append/truncate 和 opened-description release。
- **D6：FID/name 边界与 backlog 选择。** 依赖 D5 的基础机制证据；只做 negative probe、
  可选内部 dirent hook 输入模型和独立 Stage 5 候选项拆分。

可以并行的只有不共享状态真相源的节点：D1 的通用能力调查可以与 D2 的内部类型草稿并行；
LTP/helper 分类 agent 可以与实现并行读源码。D3、D4、D5 不能在缺少各自根依赖时用
fanotify 私有桥强行推进。

## 总控 Agent 使用方式

建议启动一个总控 agent 负责 orchestration，但不要让它自由决定新的协议拆分。
总控 agent 的权限边界是：

- 可以执行前置检查、代码搜索和构建级 gate。
- 可以启动只读 explorer / reviewer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同；需要扩大
  write set 时，先记录原因、范围、contract/gate 影响和批准结果。
- 可以串行集成 worker diff。
- 可以在实现开始前建立并维护事务 devlog。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。
- 遇到停止条件时回报用户，不自行拍板。

总控第一轮不要一次性派发所有 worker。建议流程是：

1. 重新确认当前分支、工作区状态、RFC 文档、背景材料和是否已建立事务日志。
2. 派发 Agent 0 做只读前置审计，并把 D1 到 D6 的当前代码落点列成依赖表。
3. 若 D1 能力缺失，优先派发 Agent 1 处理 task/fd 通用能力；同时可让 Agent 2 只做
   fanotify 内部类型和 ABI parser 草稿，但不得公开成功 gate。
4. 集成 Agent 1 + Agent 2，进行 Gate A review；只有 Stage 0+1 同时闭合后，才记录
   `fanotify_init(FAN_CLASS_NOTIF, ...)` 的公开成功证据。
5. 派发 Agent 3 实现 mark registry 与 `fanotify_mark()`，随后进行 Gate B review。
6. 派发 Agent 4 实现 path-fd read 提交协议、event fd no-notify 和 fd rollback 证据，
   随后进行 Gate C review。
7. 派发 Agent 5 接入 VFS 事件和 opened-description release，随后进行 Gate D review。
8. 派发 Agent 6 做 FID/name negative gate、LTP 分类和可选 backlog 候选拆分。
9. 派发 Agent 7 做旁路审计、构建 gate、验证证据整理和事务日志收口。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 fanotify 的总控 agent，阅读
docs/src/rfcs/fanotify/index.md、
docs/src/rfcs/fanotify/invariants.md、
docs/src/rfcs/fanotify/implementation.md、
docs/src/rfcs/fanotify/tracking-issues.md、
docs/src/rfcs/fanotify/backgrounds/index.md 和
docs/src/rfcs/fanotify/backgrounds/agent-orchestration.md。

目标是按 RFC gate 实现 fanotify 子系统：先建立 task/fd 通用能力与 fs::fanotify
owner module，使 Stage 0+1 合并 gate 闭合；再实现 mark registry、path-fd read
提交协议、VFS 基础事件 hook 和 FID/name negative gate。fanotify 是完整新子系统，
不要按一个文档阶段一个 worker 机械拆分；必须按 agent-orchestration.md 的依赖拓扑、
write set 和 review gate 分工。

你可以启动子 agent。未经批准不允许 worker 越界修改；如果更好的设计需要扩大 write
set，先提交原因、范围、contract/gate 影响和建议验证。你不是独自在代码库里工作；不得
revert 用户或其他 agent 的改动。实现开始前需要建立并维护对应事务 devlog。

第一步只做前置检查、刷新当前代码落点和准备启动的 agent 列表。不要直接一次性启动所有
worker。遇到停止条件时停止并向用户报告，不要自行拍板。
```

## Agent 0：前置审计

职责：只读审计当前代码落点是否仍符合 RFC 假设，不改代码。

读取范围：

- `anemone-abi/src/fs.rs`
- `anemone-abi/src/syscall/riscv.rs`
- `anemone-abi/src/syscall/loongarch.rs`
- syscall dispatch 表所在文件。
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/api/read_write/`
- `anemone-kernel/src/fs/api/openat.rs`
- `anemone-kernel/src/fs/api/close/`
- `anemone-kernel/src/fs/api/truncate/`
- `anemone-kernel/src/fs/iomux.rs`
- `anemone-kernel/src/fs/anonymous/`
- `anemone-kernel/src/fs/{inode,dentry,mount,filesystem}.rs`
- `anemone-kernel/src/fs/proc/tgid/fd.rs`
- `anemone-apps/user-test/ltp/groups/fanotify.txt`

检查项：

- fanotify 是否仍只有 syscall number，没有 ABI constants、syscall handler 或
  `fs::fanotify` module。
- `FileDesc::read` / readv path 是否能把当前 opened file description 的
  `FileStatusFlags` 传给 fanotify group fd read；如果不能，标成 D1 blocker。
- task/fd 层是否已有 reserved fd slot、commit/rollback、release callback 或类似机制。
- opened file description 是否能保存 fanotify close snapshot 和 notification
  suppression 标记，而不暴露 fanotify concrete type。
- anonymous inode / anony open 是否仍适合作为 group fd 承载点。
- `PollRequest` / `PollRegisterResult` / `LatchTrigger` 当前是否能支撑 group fd poll。
- VFS open/read/write/truncate/close 的集中事件点是否仍在 RFC 预期位置。
- `/proc/<tgid>/fd` 与 fdinfo 当前边界，防止 worker 把 Stage 5 fdinfo 误并入基础 gate。
- LTP fanotify profile 与背景报告中的首批/暂缓分类是否仍一致。

交付：

- 是否允许进入 Agent 1 / Agent 2 的结论。
- D1 到 D6 的当前代码落点和 blocker 表。
- 当前 repo 是否已有事务 devlog；若没有，提醒总控在实现开始前创建。
- 如果发现已经有人实现了 fanotify 局部能力，按 D1-D6 分类它属于哪个依赖节点，不能直接
  按阶段号归类。

停止条件：

- 当前分支已经存在绕过 `fs::fanotify` owner 的 fanotify private state downcast。
- `fanotify_init()` 已经可成功但 group fd read/poll/close 仍未闭合。
- task/fd 层只能通过 fanotify 私有 fd number 状态实现 event fd 或 close release。
- VFS hook 已经在 mutating lock 内打开 event fd、copy user buffer 或等待用户回复。

## Agent 1：D1 Task/Fd 通用能力

职责：建立 fanotify 下游共享的 fd/task 基础能力，不写 fanotify feature policy。

write set：

- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/fs/api/read_write/mod.rs`
- `anemone-kernel/src/fs/api/read_write/read.rs`
- `anemone-kernel/src/fs/api/read_write/readv.rs`
- `anemone-kernel/src/fs/api/close/close.rs`
- `anemone-kernel/src/fs/api/close/close_range.rs`
- `anemone-kernel/src/fs/api/close/mod.rs`
- `anemone-kernel/src/fs/anonymous/mod.rs`
- `anemone-kernel/src/fs/anonymous/anony_fs.rs`
- 实现开始后对应的事务 devlog。

语义要求：

- read/readv fanotify group fd 路径能观察当前 opened file description
  `FileStatusFlags`，`F_SETFL(O_NONBLOCK)` 后续变更必须影响 empty read。
- 提供 fd reservation / commit / rollback helper，reserved slot 在 commit 前不被普通
  fd 分配或 close 观察；commit 不再分配、不阻塞、不可失败；rollback 幂等释放。
- 提供 opened-description release callback 或等价语义 close hook；不能用 fd table slot
  close 冒充最后 release。
- 提供通用 notification suppression 标记，供 fanotify event fd 后续 I/O / close 跳过
  fanotify enqueue；task/fd 核心不能检查 fanotify concrete type。
- 若需要 read-user typed operation，syscall 层只做 generic dispatch，不写
  fanotify-specific downcast 分支。

验证：

```bash
just build
git diff --check
```

Gate A 前 reviewer 检查：

- D1 不保存 fanotify 专用 fd number 状态。
- reservation state 与 fd table open-state 是单一真相源。
- release hook 能覆盖 dup/fork shared opened description 的最后 release。
- notification suppression 是通用 file/opened-description 能力。

## Agent 2：D2 Fanotify Owner Module 与 Stage 0+1 Gate

职责：建立 `fs::fanotify` owner module、UAPI/parser、syscall facade、group fd 和
bounded queue；与 Agent 1 合流后形成首个公开 gate。

write set：

- `anemone-abi/src/fs.rs`
- `anemone-kernel/src/fs/mod.rs`
- 新增 `anemone-kernel/src/fs/fanotify/mod.rs`
- 新增 `anemone-kernel/src/fs/fanotify/types.rs`
- 新增 `anemone-kernel/src/fs/fanotify/api/mod.rs`
- 新增 `anemone-kernel/src/fs/fanotify/api/init.rs`
- 新增 `anemone-kernel/src/fs/fanotify/api/mark.rs`
- 新增 `anemone-kernel/src/fs/fanotify/group.rs`
- 新增 `anemone-kernel/src/fs/fanotify/file.rs`
- 新增 `anemone-kernel/src/fs/fanotify/event.rs`
- 新增 `anemone-kernel/src/fs/fanotify/queue.rs`
- 新增 `anemone-kernel/src/fs/fanotify/registry.rs`
- 新增 `anemone-kernel/src/fs/fanotify/mark.rs`
- 新增 `anemone-kernel/src/fs/fanotify/hooks.rs`
- `anemone-kernel/src/fs/anonymous/mod.rs`
- `anemone-kernel/src/fs/anonymous/anony_fs.rs`
- 实现开始后对应的事务 devlog。

禁止写入：

- 不新增 `anemone-kernel/src/fs/api/fanotify/`；fanotify syscall API 归
  `anemone-kernel/src/fs/fanotify/api/`。
- 不改 `anemone-kernel/src/fs/api/mod.rs` 来承载 fanotify 逻辑；如果宏注册需要模块可达，
  通过 `fs/mod.rs` 引入 `mod fanotify;`。

语义要求：

- `fs::fanotify` 当前阶段必须落下完整骨架：`mod.rs`、`types.rs`、
  `api/{mod,init,mark}.rs`、`group.rs`、`file.rs`、`event.rs`、`queue.rs`、
  `registry.rs`、`mark.rs`、`hooks.rs`。`registry.rs`、`mark.rs`、`hooks.rs` 可以先是
  fail-closed / empty facade，但文件边界必须存在，避免后续 worker 临时新建旁路目录。
- `api/init.rs` 和 `api/mark.rs` 持有 `#[syscall(...)]` handler；`fs/api` 不承载 fanotify
  parser 或 errno matrix。
- 只有 `mod.rs` facade 对外暴露 syscall/VFS/task-fd 需要的 typed API。
- Linux UAPI constants 和 `repr(C)` struct 只停在 ABI/parser/read-copy 边界；group 内部
  使用 `FanMask`、`FanGroupMode`、`FanEventKind` 等语义类型。
- `fanotify_init()` parser 固定 init flag matrix；暂缓 FID/name、pidfd、audit、
  unlimited queue/marks 等默认 fail closed。
- group fd vtable 完整 fail closed：普通 read/readv 消费 queue，poll 观察 readiness，
  optional ioctl 只处理 group fd 命令；seek/read_at/write_at/read_dir 不消费 queue。
- Stage 1 synthetic event 使用 `fd = FAN_NOFD`，不提前打开 path-fd event object。
- queue cap 与 overflow sentinel 在首个真实 enqueue 前已经存在。
- close/dead 唤醒 blocking read / poll waiters，并清理 queue/marks 的语义入口。

验证：

```bash
just build
git diff --check
```

额外证据：

- `fanotify_init(FAN_CLASS_NOTIF, valid_event_f_flags)` 只在 Stage 0+1 合并 gate 后成功。
- empty nonblock read 返回 `EAGAIN`；blocking read 可被 synthetic enqueue 或 close 唤醒。
- poll empty register、enqueue 后 wake、close wake 均有源码级或最小 smoke 证据。
- unsupported init flags 有稳定 errno 证据。

Gate A reviewer 检查：

- Stage 0 没有被单独记为用户可见 LTP gate。
- syscall/VFS/task-fd 层没有 downcast fanotify private state。
- read nonblock 依赖 current `FileStatusFlags`，不是 fanotify private mirror。
- group lifecycle 和 bounded queue 满足 `invariants.md` 的闭合条件。

## Agent 3：D3 Mark Registry 与 `fanotify_mark()`

职责：实现 registry、mark identity、basic mark parser 和 add/remove/flush。

write set：

- `anemone-kernel/src/fs/fanotify/mod.rs`
- `anemone-kernel/src/fs/fanotify/types.rs`
- `anemone-kernel/src/fs/fanotify/api/mark.rs`
- `anemone-kernel/src/fs/fanotify/group.rs`
- `anemone-kernel/src/fs/fanotify/registry.rs`
- `anemone-kernel/src/fs/fanotify/mark.rs`
- `anemone-kernel/src/fs/namei.rs`
- `anemone-kernel/src/fs/path.rs`
- `anemone-kernel/src/fs/dentry.rs`
- `anemone-kernel/src/fs/inode.rs`
- `anemone-kernel/src/fs/mount.rs`
- `anemone-kernel/src/fs/filesystem.rs`
- 实现开始后对应的事务 devlog。

禁止写入：

- 不新增或使用 `anemone-kernel/src/fs/api/fanotify/`。
- 不在 `fs/namei.rs`、`fs/path.rs`、`fs/dentry.rs`、`fs/inode.rs`、`fs/mount.rs` 或
  `fs/filesystem.rs` 中保存 fanotify mark 状态；这些文件只能提供必要的身份、path
  resolution 或 target-lifecycle helper。

语义要求：

- 定义 `FanTargetKey::{Inode, Mount, SuperBlock}` 或等价 key，集中封装相等/hash。
- registry 是 mark record 的唯一 owner；target map 和 group cleanup list 只保存
  `MarkHandle`，不得复制 event mask、ignored mask、target refs、`target_dead` 或
  generation。
- registry 不强拥有 group，只保存 group id/generation 和 weak/non-owning handle。
- `fanotify_mark(ADD/REMOVE/FLUSH)` validation 在进入 registry 前完成；unsupported mask
  不得空成功。
- 支持首批 inode/mount/filesystem mark、`FAN_MARK_ONLYDIR`、`FAN_MARK_DONT_FOLLOW`、
  `FAN_EVENT_ON_CHILD`、`FAN_ONDIR` 和 legacy basic ignored mask。
- `FAN_MARK_INODE == 0` 的 parser 规则显式存在。
- ADD / REMOVE / FLUSH 与后续 matching 共享 registry lock 线性化策略。

验证：

```bash
just build
git diff --check
```

额外证据：

- `fanotify04` 的 `ONLYDIR`、`DONT_FOLLOW`、`FLUSH` 基础分支，或等价定向 smoke。
- mark add/remove/flush 只影响目标 group。
- unsupported mark masks 返回稳定 errno。

Gate B reviewer 检查：

- mark state 没有第二份真相源。
- target lifecycle 采用 RFC 接受的强引用 + pre-unmount flush / mark-dead 口径。
- close/flush/remove cleanup handles 不会删到其他 group。
- `FAN_MARK_IGNORE` 新语义没有被 legacy ignored mask worker 偷偷接受。

## Agent 4：D4 Path-Fd Read 提交协议

职责：实现 read-user path、event fd reservation/commit/rollback、metadata copyout 和
no-notify，不接入真实 VFS hook。

write set：

- `anemone-kernel/src/fs/fanotify/mod.rs`
- `anemone-kernel/src/fs/fanotify/types.rs`
- `anemone-kernel/src/fs/fanotify/group.rs`
- `anemone-kernel/src/fs/fanotify/file.rs`
- `anemone-kernel/src/fs/fanotify/event.rs`
- `anemone-kernel/src/fs/fanotify/queue.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/fs/api/read_write/mod.rs`
- `anemone-kernel/src/fs/api/read_write/read.rs`
- `anemone-kernel/src/fs/api/read_write/readv.rs`
- `anemone-kernel/src/fs/anonymous/mod.rs`
- `anemone-kernel/src/fs/anonymous/anony_fs.rs`
- `anemone-kernel/src/fs/mod.rs`
- 实现开始后对应的事务 devlog。

语义要求：

- `read(fanotify_fd)` 不复用 “生成 kernel buffer 再 generic copyout” 的普通 read 事务；
  必须有可审查的 fanotify read-user 提交协议。
- group lock 下选择完整 event 并出队；释放锁后构造 metadata 和打开 path-fd object。
- event fd 使用 D1 的 reserved slot；metadata copyout 成功后 commit，失败时 rollback。
- 一个 metadata record 不得半提交；partial read 只能返回已提交完整 records 的字节数。
- object open 失败时输出 `FAN_NOFD` 或 RFC 记录的稳定策略，不 panic。
- `NoNotifyGuard` 只包住 fanotify 内部 event-fd helper；返回用户的 event fd 带通用
  notification suppression 标记。

验证：

```bash
just build
git diff --check
```

额外证据：

- synthetic path-fd event 的 metadata copyout 成功能发布 event fd。
- copyout 失败不会泄漏用户不可见 fd。
- event fd 后续 read/write/close 不递归产生 fanotify event。

Gate C reviewer 检查：

- sys_read/readv 没有 fanotify concrete type special-case。
- fd reservation/commit/rollback 的 owner 是 task/fd 层。
- group lock、registry lock 不包用户 copy 或 VFS open。
- no-notify 标记不传播到普通 user open。

## Agent 5：D5 VFS 基础 Event Injection

职责：把基础 open/read/write/close 事件接入 fanotify queue。

write set：

- `anemone-kernel/src/fs/fanotify/mod.rs`
- `anemone-kernel/src/fs/fanotify/types.rs`
- `anemone-kernel/src/fs/fanotify/event.rs`
- `anemone-kernel/src/fs/fanotify/queue.rs`
- `anemone-kernel/src/fs/fanotify/registry.rs`
- `anemone-kernel/src/fs/fanotify/mark.rs`
- `anemone-kernel/src/fs/fanotify/hooks.rs`
- `anemone-kernel/src/fs/api/openat.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/fs/api/read_write/mod.rs`
- `anemone-kernel/src/fs/api/read_write/read.rs`
- `anemone-kernel/src/fs/api/read_write/readv.rs`
- `anemone-kernel/src/fs/api/read_write/pread64.rs`
- `anemone-kernel/src/fs/api/read_write/preadv.rs`
- `anemone-kernel/src/fs/api/read_write/write.rs`
- `anemone-kernel/src/fs/api/read_write/writev.rs`
- `anemone-kernel/src/fs/api/read_write/pwrite64.rs`
- `anemone-kernel/src/fs/api/read_write/pwritev.rs`
- `anemone-kernel/src/fs/api/read_write/pwritev2.rs`
- `anemone-kernel/src/fs/api/truncate/truncate.rs`
- `anemone-kernel/src/fs/api/truncate/ftruncate.rs`
- `anemone-kernel/src/fs/api/truncate/mod.rs`
- `anemone-kernel/src/fs/inode.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/mod.rs`
- 实现开始后对应的事务 devlog。

禁止写入：

- 不在具体 backend 文件如 `anemone-kernel/src/fs/ext4/file.rs`、
  `anemone-kernel/src/fs/ramfs/file.rs`、`anemone-kernel/src/fs/proc/` 或
  `anemone-kernel/src/fs/devfs/` 中散落 fanotify policy。若某个 backend 确实缺少统一
  VFS 成功点，先申请 write set 扩展并说明为什么不能在 `fs/file.rs` / `fs/inode.rs`
  层解决。

语义要求：

- open 成功后派发 `FAN_OPEN`。
- `File::{read,read_at}` 成功读取后派发 `FAN_ACCESS`；hook 位于 fd/VFS gate 后、
  backend 成功返回后，不下沉到具体 backend `FileOps::{read_at,write_at}`。
- write/write_at/append/truncate 等成功内容修改后派发 `FAN_MODIFY`，并服务 legacy
  ignored mask modify clearing。
- open 成功时在 opened file description 上记录 close snapshot：`PathRef` / `FanPathKey`
  和打开时 access mode 是否具备写能力。
- 最后 opened-description release 时派发 `FAN_CLOSE_WRITE` 或 `FAN_CLOSE_NOWRITE`；
  dup/fork shared description 只产生一次 close event。
- matching 支持 inode/mount/filesystem、self、parent + `FAN_EVENT_ON_CHILD`、`FAN_ONDIR`
  和 basic ignore mask。
- hook 只生成 queue item，不打开 fd、不 copy user buffer、不等待 permission response。

验证：

```bash
just build
git diff --check
```

额外证据：

- `fanotify01` 非 FID 基础路径。
- `fanotify02` 基础 child open/access/modify/close。
- event fd 可 readlink 到目标对象或至少符合 event flags 的最小手工证据。
- read/write 失败不生成成功事件。

Gate D reviewer 检查：

- `FAN_CLOSE_*` 来源是 opened file description release；temporary bridge 不得作为退出证据。
- ADD / REMOVE / FLUSH 与 matching snapshot / queue append 线性化无歧义。
- VFS hook 不访问 fanotify registry/group internals。
- queue cap、overflow sentinel、dead group/target checks 在真实 enqueue 前生效。

## Agent 6：D6 FID/Name Negative Gate 与 Backlog 分类

职责：在基础 path-fd 通知闭合后，验证暂缓项不会伪成功，并按失败日志拆独立 backlog。

write set：

- `anemone-kernel/src/fs/fanotify/mod.rs`
- `anemone-kernel/src/fs/fanotify/types.rs`
- `anemone-kernel/src/fs/fanotify/api/init.rs`
- `anemone-kernel/src/fs/fanotify/api/mark.rs`
- `anemone-kernel/src/fs/fanotify/event.rs`
- `anemone-kernel/src/fs/fanotify/hooks.rs`
- 可选 dirent hook 输入模型文件：
  - `anemone-kernel/src/fs/api/mkdirat.rs`
  - `anemone-kernel/src/fs/api/unlinkat.rs`
  - `anemone-kernel/src/fs/api/renameat2.rs`
  - `anemone-kernel/src/fs/api/linkat.rs`
  - `anemone-kernel/src/fs/api/symlinkat.rs`
- `anemone-apps/user-test/ltp/groups/fanotify.txt` 仅在用户明确要求调整 profile 时修改。
- 实现开始后对应的事务 devlog。

禁止写入：

- 不新增 `fs/api/fanotify/`。
- 不新增正向 FID/name userspace ABI 文件或 procfs fdinfo 输出，除非先更新 RFC 或创建
  follow-up gate。

语义要求：

- FID/name report flags 继续拒绝；不得输出不完整 info record。
- `FAN_CREATE`、`FAN_DELETE`、`FAN_MOVED_FROM`、`FAN_MOVED_TO`、`FAN_RENAME` 等正向
  name 类事件仍移入 FID/name follow-up。
- `FAN_ATTRIB`、`FAN_DELETE_SELF`、`FAN_MOVE_SELF` 只有在 path-fd observable ABI 已
  明确兼容时才能单独提前。
- 可选 dirent hook 只准备 parent/name/old/new 输入模型，不作为用户可见 ABI 证据。
- Stage 5 候选项必须单独拆 gate：`FAN_UNLIMITED_QUEUE`、完整 ignore mask、`FAN_REPORT_TID`、
  group/mark limit、fdinfo、`FAN_OPEN_EXEC` 等不能混入基础闭合。

验证：

```bash
just build
git diff --check
```

额外证据：

- 自建或裁剪 probe 校验 FID/name、pidfd、permission、FS error、evictable mark 等暂缓项
  返回稳定 errno。
- stock `fanotify14` 仍按 RFC 归入 FID/name 暂缓，不作为非 FID 负例直接验收。

Gate E reviewer 检查：

- 没有 silent success 的暂缓 feature flag。
- LTP 失败分类区分基础机制缺陷、helper TCONF、stock 暂缓失败和 Stage 5 候选项。
- 新增 feature 如果改变不变量，必须先回到 RFC 或新增 follow-up。

## Agent 7：旁路审计与收口

职责：只读审计集成后的旁路、文档/事务日志、构建和验证证据；只在收口文档中写入事实。

读取范围：

- `docs/src/rfcs/fanotify/`
- 对应事务 devlog。
- `anemone-kernel/src/fs/fanotify/`
- `anemone-kernel/src/fs/`
- `anemone-kernel/src/task/files.rs`
- `anemone-abi/src/`
- `anemone-apps/user-test/ltp/groups/fanotify.txt`

旁路搜索：

```bash
rg -n "fanotify|fsnotify|FAN_" anemone-kernel anemone-abi
rg -n "after_modified|ModifType|vfs_touch|vfs_mkdir|vfs_unlink|vfs_rmdir|vfs_rename|finish_open|close_fd" anemone-kernel/src/fs anemone-kernel/src/task
rg -n "PollRequest|PollRegisterResult|LatchTrigger|poll:" anemone-kernel/src
rg -n "FIONREAD|fdinfo|pidfd|name_to_handle|open_by_handle" anemone-kernel anemone-abi
```

交付：

- 每个 gate 的通过/失败和验证证据。
- 未跑验证、用户跑验证和 agent 跑验证分开记录。
- 仍然暂缓的 LTP 分支按 RFC 分类写入事务 devlog，不写成 open design issue。
- 如果基础机制已闭合，给出剩余 Stage 5 backlog 的建议启动顺序。

验证：

```bash
just build
git diff --check
```

停止条件：

- 发现 syscall、VFS、task/fd 或 procfs 层 downcast fanotify private state。
- 发现 `fanotify_init()` 可成功但 Stage 0+1 合并 gate 证据缺失。
- 发现基础 gate 依赖 temporary fd-close bridge、private nonblock mirror 或 private fd
  reservation 真相源。
- 发现暂缓 feature 返回成功但没有真实 userspace 可观测语义。
