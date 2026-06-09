# fanotify 迁移实施计划

**状态：** Draft
**最后更新：** 2026-06-09
**父 RFC：** [RFC-20260604-fanotify](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文按可提交、可验证的阶段拆分。每个阶段完成后应能 `just build`，功能阶段还应有最小 LTP 或用户态验证证据。

Stage 0 是内部 ABI/probe checkpoint，不是独立用户可见 LTP gate。首个可公开验收的 gate 是 Stage 0 + Stage 1：`fanotify_init()` 成功返回 group fd 时，group fd 的 blocking/nonblock read、poll、close wakeup 和最小 queue 行为必须已经闭合。

## 迁移原则

- Linux ABI 常量、结构体和 flag parser 只放在 `anemone_abi`、`fs/fanotify/api/` 和 fanotify fd copy 边界。
- fanotify 内部长期状态使用语义类型，不直接保存 Linux UAPI struct。
- `fs::fanotify` 必须作为 owner 模块隐藏 group、registry、queue、mark record、lock 和 file private state；syscall dispatch / VFS / task-fd 层只通过 `mod.rs` facade 暴露的 typed API 交互。fanotify syscall API 必须位于 `anemone-kernel/src/fs/fanotify/api/`，不新增 `anemone-kernel/src/fs/api/fanotify/`。
- fanotify group fd 的 `AnyOpaque` downcast 只允许发生在 fanotify 自己的 file ops / helper 内，不能让 syscall、VFS hook 或 task/fd 层识别 fanotify private state。
- 先实现 privileged path-fd notification；FID/name、pidfd、permission events、FS error、evictable marks 和 exec-open events 暂缓。
- unsupported feature 必须返回稳定 errno，不得空成功。
- event enqueue 不做用户态 copy，不打开事件 fd，不阻塞。
- poll/read readiness 使用 typed `PollRequest` / `PollRegisterResult` 与 `LatchTrigger`；不得重新引入 busy-poll waiter。
- 首个用户可见 group fd 不允许 empty blocking read 返回 `EAGAIN`；nonblock 判断必须读取当前 opened file description 的 `FileStatusFlags`，使 `F_SETFL(O_NONBLOCK)` 后续变更生效。
- fanotify group fd 需要完整实现当前 `FileOps` vtable：普通 `read` / readv 系列进入 fanotify read-user 路径，`poll` 使用 latch readiness，`ioctl` 只处理可选 group fd 命令；`seek`、`read_at`、`write_at` 和 `read_dir` 必须 fail closed，不能消费 group queue、创建 event metadata fd 或改变 fanotify 状态。
- 首个真实 VFS enqueue 前必须已有固定 queue cap 和 overflow sentinel；无界队列或只统计不丢弃都不能进入 path-fd event 阶段。
- 首批 legacy basic ignore mask 只覆盖 `FAN_MARK_IGNORED_MASK` / `FAN_MARK_IGNORED_SURV_MODIFY` 的 add/remove/modify-survive 语义；新的 `FAN_MARK_IGNORE` 和跨 inode/mount 合并规则进入独立 backlog gate。
- fd reservation / commit / rollback 和 opened-description release 由 task/fd 层拥有，fanotify 只能调用受控 helper 或接收 release 回调，不维护并行 fd-table 状态或用户 fd number 身份。
- event-fd no-notify 标记必须落为通用 file/opened-description notification suppression 能力，不让 task/fd 核心依赖 fanotify 具体类型。
- 每个阶段都保持现有 VFS、pipe、iomux 和 credentials 行为不退化。

## 阶段 0：UAPI 与 syscall probe（内部 checkpoint）

目标：建立 syscall ABI parser 和 feature gate 矩阵，让 LTP feature probe 进入可控分支；本阶段单独存在时不作为 LTP gate。

前置条件：

- 本 RFC 的暂缓范围已接受。

交付：

- 在 `anemone_abi::fs::linux::fanotify` 中新增 fanotify 常量和 `repr(C)` ABI struct：
  - `fanotify_event_metadata`
  - `fanotify_response`
  - 基础 `FAN_*` init flags、mark flags、event masks
  - `FAN_EVENT_METADATA_LEN`、`FAN_NOFD`、metadata version
- 新增 `fs::fanotify` owner module skeleton，固定文件骨架：
  - `anemone-kernel/src/fs/fanotify/mod.rs`
  - `anemone-kernel/src/fs/fanotify/types.rs`
  - `anemone-kernel/src/fs/fanotify/api/mod.rs`
  - `anemone-kernel/src/fs/fanotify/api/init.rs`
  - `anemone-kernel/src/fs/fanotify/api/mark.rs`
  - `anemone-kernel/src/fs/fanotify/group.rs`
  - `anemone-kernel/src/fs/fanotify/file.rs`
  - `anemone-kernel/src/fs/fanotify/event.rs`
  - `anemone-kernel/src/fs/fanotify/queue.rs`
  - `anemone-kernel/src/fs/fanotify/registry.rs`
  - `anemone-kernel/src/fs/fanotify/mark.rs`
  - `anemone-kernel/src/fs/fanotify/hooks.rs`
- `fs/fanotify/api/{init,mark}.rs` 封装 `fanotify_init()` / `fanotify_mark()` 的 Linux ABI parser、errno matrix、feature gate、path resolution 入口和 facade 调用；不得新增 `fs/api/fanotify/` 作为 fanotify syscall 逻辑目录。
- `fs/api` 不新增 fanotify 子目录，也不承载 fanotify parser、errno matrix 或 group/mark 逻辑；若 syscall 宏注册需要模块可达性，通过 `fs/mod.rs` 引入 `mod fanotify;` 和 `fs/fanotify/mod.rs` 内部 wiring 完成。
- `fs::fanotify::mod` 是唯一模块外 facade，暴露 syscall/VFS/task-fd 需要的 typed API；`group.rs`、`file.rs`、`queue.rs`、`registry.rs`、`mark.rs`、`event.rs` 和 `hooks.rs` 的内部 storage 不直接对模块外公开。
- 新增 `fanotify_init()` syscall handler。
- 新增 `fanotify_mark()` syscall handler。
- `fanotify_init(FAN_CLASS_NOTIF, valid_event_f_flags)` 的公开成功推迟到 Stage 0 + Stage 1 合并 gate；若 Stage 0 单独提交，只能作为内部 probe checkpoint。
- `FAN_CLOEXEC` 映射到 fd flags；`FAN_NONBLOCK` 映射到 opened file description 的 file status flags。后续 read 行为必须读取当前 `FileStatusFlags`，使 `F_SETFL(O_NONBLOCK)` 生效。若实现期只能通过类似 pipe 的同步桥把 `FileStatusFlags::NONBLOCK` 镜像到 fanotify private state，该桥只能作为 temporary compatibility bridge 记录，不能作为 Stage 0 + Stage 1 合并 gate 的闭合证据。
- `FAN_CLASS_CONTENT` / `FAN_CLASS_PRE_CONTENT` 可以先创建通知类 group 以支撑 permission feature probe，但 permission masks 在 mark 阶段返回 `EINVAL`；permission response `write()` 在 permission gate 前也必须返回稳定 unsupported / invalid errno，不能接受无 pending event 的假回复。
- 默认拒绝 FID/name、pidfd、unprivileged FID-only、audit、unlimited queue/marks 等暂缓 init flags。
- 对非法 `event_f_flags`、非法 class 组合和未知 bits 返回 `EINVAL`。

Stage 0 init flag matrix：

| flag / 组合 | Stage 0 策略 | errno / LTP 分类 | 后续 gate |
| --- | --- | --- | --- |
| `FAN_CLASS_NOTIF` | 接受 parser；公开成功只能在 Stage 0 + Stage 1 合并 gate 宣称。 | 合并 gate 后支撑 `fanotify01/02/04/08` 基础 setup；Stage 0 单独落地不得作为 LTP 成功证据。 | Stage 0 + Stage 1 |
| `FAN_CLOEXEC` | 接受，映射到 group fd `CLOSE_ON_EXEC`。 | `fanotify08` 或等价 fcntl probe。 | Stage 0 + Stage 1 |
| `FAN_NONBLOCK` | 接受，映射到 group fd 初始 file status flags；`read()` 每次读取当前 opened file description status flags，而不是 fanotify private 副本。 | empty nonblock read 返回 `EAGAIN`；`F_SETFL(O_NONBLOCK)` 后续变更必须生效。 | Stage 0 + Stage 1 |
| valid `event_f_flags` | 接受并保存为 event object fd 创建模板，只允许外部 open status/access flags。 | invalid bits 或非法 access mode 返回 `EINVAL`。 | Stage 3 path-fd read |
| `FAN_CLASS_CONTENT` / `FAN_CLASS_PRE_CONTENT` | 接受通知类 group creation，避免 permission 用例在 `SAFE_FANOTIFY_INIT()` 处 TBROK；不创建 pending permission queue。 | permission mask 在 `fanotify_mark()` 返回 `EINVAL`，让 permission feature probe TCONF；permission response `write()` 在 follow-up 前返回稳定 unsupported / invalid errno。 | permission follow-up |
| FID/name report flags | 拒绝：`FAN_REPORT_FID`、`FAN_REPORT_DIR_FID`、`FAN_REPORT_NAME`、`FAN_REPORT_TARGET_FID` 均返回 `EINVAL`。 | helper-based probe 归类为 unsupported；stock FID 用例属于暂缓清单。 | FID/name follow-up |
| `FAN_REPORT_PIDFD` | 拒绝 `EINVAL`。 | pidfd 用例归入暂缓。 | pidfd follow-up |
| `FAN_REPORT_TID` | 拒绝 `EINVAL`，除非同阶段补齐 metadata `pid` 的 tid 语义。 | `fanotify11` 归入 Stage 5 候选。 | Stage 5 |
| `FAN_UNLIMITED_QUEUE` | 首批拒绝 `EINVAL`，不得忽略成功；limited queue 仍使用 Stage 1 cap + overflow sentinel。 | stock `fanotify05` unlimited 分支不作为首批通过目标；若完整 LTP 运行出现该失败，事务日志按 unsupported init flag 分类。 | Stage 5 / resource-limit gate |
| `FAN_UNLIMITED_MARKS` | 首批拒绝 `EINVAL`，不得忽略成功。 | group/mark limit 用例归入 Stage 5。 | Stage 5 / resource-limit gate |
| `FAN_ENABLE_AUDIT` | 无 audit backend 时拒绝 `EINVAL`。 | audit 相关行为暂缓。 | audit follow-up |
| unprivileged pure path-fd listener | 拒绝 `EPERM`；不创建不完整 listener。 | 事务日志按权限边界分类，不承诺 stock LTP 一定 TCONF。 | FID/unprivileged follow-up |
| unknown bits / invalid class combination | 拒绝 `EINVAL`。 | ABI validation probe 应看到稳定 invalid input。 | Stage 0 |

审计：

- 确认 syscall handler 不保存 Linux ABI struct 到 group state。
- 确认 syscall handler 只调用 fanotify typed facade，不 downcast fanotify group fd private state。
- 确认失败路径不泄漏 fd 或 group state。
- 确认 `FAN_CLASS_CONTENT` probe 不会在 `SAFE_FANOTIFY_INIT()` 处 TBROK permission 用例。
- 确认本阶段单独落地时不会被记录为 fanotify 用户可见成功 gate。
- 确认 `FAN_NONBLOCK` 不被固化到 fanotify group private state；若存在临时同步桥，事务日志必须标注为 bridge，并在 Stage 1 合并 gate 前替换为 current-status-flags 可复审路径。

验证：

- `just build`
- init flag matrix 的源码级或最小用户态 probe。
- 若 Stage 0 + Stage 1 同批验收：`fanotify_init(FAN_CLASS_NOTIF, O_RDONLY)` 成功。
- 若 Stage 0 + Stage 1 同批验收：`fanotify08` 或等价 fcntl 检查 `FAN_CLOEXEC`。
- 高阶 init flag 返回预期 `EINVAL` / `EPERM`。

退出条件：

- ABI/parser 和暂缓项 errno 策略有可复审证据。
- LTP fanotify 基础 probe 是否不再因 `ENOSYS` 全组跳过，只能在 Stage 0 + Stage 1 合并 gate 中声明。
- 暂缓项不会伪成功。

## 阶段 1：group fd、blocking readiness 与 bounded event queue

目标：建立可读、可 poll、可关闭的 fanotify group fd。

前置条件：

- 阶段 0 完成。
- `fs::fanotify` owner module 边界确定，global registry、group lock、queue、mark record 和 file private state 均不被模块外直接访问。
- read/readv 系列已经能通过 typed read dispatch、FileDesc-aware read context 或等价机制让 fanotify group fd read 观察当前 opened file description `FileStatusFlags`；Stage 0 + Stage 1 合并 gate 不能依赖 fanotify private nonblock 副本。
- task/fd 层 semantic release hook 或等价 opened-description release 机制已确定；fanotify 不维护独立 fd table 或用户 fd number 计数。
- 首个用户可见 gate 采用 Stage 0 + Stage 1 合并验收。

交付：

- 使用 anonymous inode 创建 fanotify group fd。
- group state 包含：
  - stable `GroupId` / generation
  - init mode / class
  - event fd flags
  - event queue
  - fixed queue cap 与 drop/overflow 计数
  - poll trigger queue
  - group-owned `MarkHandle` 列表；不得保存独立 `FanMark` 副本
  - task/fd 层 semantic release hook、opened-description release callback 或等价机制
  - dead/closing 状态
- 实现 group fd `read()`：
  - empty + blocking：等待，并能被 enqueue / close / dead-group 唤醒。
  - empty + nonblock：按当前 opened file description `FileStatusFlags` 返回 `EAGAIN`，`F_SETFL(O_NONBLOCK)` 的后续变化必须即时生效。
  - buffer 太小：返回 `EINVAL`。
  - 输出 `fanotify_event_metadata`。
  - Stage 1 synthetic event 使用 `fd = FAN_NOFD`，不创建 path-fd event object。
- 实现 group fd 其余 vtable：
  - `seek` 返回 `IllegalSeek` / `ESPIPE` 风格错误。
  - `read_at` / `write_at` 返回稳定 unsupported / illegal seek 错误，不得进入 fanotify queue read 或 permission response path。
  - `read_dir` 返回 `NotDir`。
  - `ioctl` 默认返回 `UnsupportedIoctl` / `ENOTTY`；若实现 `FIONREAD`，只在 group fd `FileOps::ioctl` 内通过 `IoctlCtx` 写回 queued bytes / records，不得新增 `sys_ioctl()` fanotify special-case。
- 实现 group fd `poll()`：
  - queue 非空返回 `READABLE`。
  - queue 为空的 register 请求保存 `LatchTrigger`。
  - enqueue 后 detach triggers 并在 lock 外 trigger。
- group fd file ops 内部可以从 `AnyOpaque` 取出 typed group state；模块外代码不得对 fanotify private state 做 cast 或按 private type 分支。
- 固定首批 queue cap：默认 `DEFAULT_MAX_EVENTS = 16384`，并在 group state 中保存 `max_events`、`overflow_queued`、`dropped_events`。队满时若当前没有 overflow sentinel，则排入单个 `FAN_Q_OVERFLOW` / `FAN_NOFD` event；若 sentinel 已在队列中，则丢弃新事件并递增 `dropped_events`。精确 sysctl、merge/order 和 `FAN_UNLIMITED_QUEUE` 后移，但资源上限和可观测 overflow 不后移。
- group fd `FIONREAD` 仍为可选；若不实现，不作为 Stage 1 验收项；若实现，必须走 fanotify group fd `FileOps::ioctl`，未知命令返回 `UnsupportedIoctl` / `ENOTTY`。
- 最后一个用户可见 descriptor / opened-description ref 关闭时执行 semantic close teardown：标记 closing/dead、清理 queue、marks、poll triggers，并唤醒 blocking read / poll；shared group state 的实际内存释放可以等 in-flight syscall transient refs 归零。
- Stage 1 必须提供 fd/task 层 release hook、table-ref 计数或等价机制来触发 semantic close teardown；该 release hook 必须由显式 close、task exit、fd-table replacement / unshare 等可睡眠、interrupts-enabled 生命周期路径调用。不得依赖 group state memory last-drop、`FilesState::drop()`、task `Drop` 或 deferred task dispose 唤醒当前正在阻塞且持有 transient ref 的 read，也不得把任意 fd number close 当作 shared group teardown。

审计：

- poll register 与 enqueue 不 lost wake。
- trigger 不在 group lock 内执行。
- close 后 late enqueue fail closed。
- queue item 不持有短生命周期引用。
- close/dead 唤醒 blocking read 和 poll waiters。
- blocking read 持有 transient ref 时，最后 descriptor close 仍能触发 semantic close wakeup。
- `Drop` 只负责内存释放或断言遗漏的显式清理；不能在 `Drop` / deferred task dispose 中调用 opened-description final-release、获取 fanotify registry 普通 `Mutex`、触发 waiters 或修改 marks。
- read 路径能观察当前 opened file description status flags；`F_SETFL(O_NONBLOCK)` 后 empty read 返回 `EAGAIN`，清除后恢复 blocking 语义。临时 private nonblock mirror 不能作为合并 gate 闭合证据。
- queue 中持有的 `PathRef` 或等价引用在解锁后释放。
- Stage 1 不打开 event object fd，不需要 no-notify guard。
- group state 不保存用户 fd number，也不保存与 task/fd 表并行的 descriptor 计数。

验证：

- `just build`
- 最小内核或用户态 smoke：创建 group，poll empty 不 ready，手工注入 `FAN_NOFD` event 后 poll/read ready。
- blocking empty read 会等待，并能被 synthetic enqueue 或 close/dead 唤醒。
- `FAN_NONBLOCK` 和 `F_SETFL(O_NONBLOCK)` 后的 empty read 返回 `EAGAIN`；清除 `O_NONBLOCK` 后 empty read 重新进入 blocking 路径。

退出条件：

- Stage 0 + Stage 1 可以作为首个用户可见 gate。
- group fd lifecycle、read/nonblock/poll/close wakeup 和 bounded queue 有可复审证据。

## 阶段 2：mark registry 与 fanotify_mark

目标：支持 basic inode / mount / filesystem mark。

前置条件：

- 阶段 1 完成。
- target identity 和 `registry -> group` 锁序已按不变量固定。
- 首批 mark target lifecycle 采用强引用 + pre-unmount flush / mark-dead：mark 和 queued event 可 pin 目标，但 umount/filesystem kill 完整兼容必须等 VFS 接入 pre-unmount 清理 hook 后才能宣称。
- registry ownership 采用单一 `FanMarkRecord` owner；target map 和 group cleanup list 只能保存 `MarkHandle`。

交付：

- 定义内部类型：
  - `FanMask`
  - `FanMarkFlags`
  - `FanMarkCommand`
  - `FanTarget::{Inode, Mount, Filesystem}`
  - `FanTargetKey::{Inode, Mount, SuperBlock}`，集中封装 pointer identity 或对象内 stable id 的相等/hash
  - `FanPathKey { mount, dentry }` 或等价 mount+dentry path identity
  - `FanMark`
- registry 区分 `marks_by_target` 与 group-owned cleanup handles：
  - `marks_by_target: HashMap<FanTargetKey, Vec<MarkHandle>>`
  - group state 保存 `mark_handles: Vec<MarkHandle>`
  - `MarkHandle` 至少包含 group id、group generation、target key、slot/generation
  - `FanMarkRecord` 只由 registry arena / slot map 或等价 owner 持有；event mask、ignored mask、target refs、`target_dead` 和 generation 不得在 group state 或 target map 中复制
  - registry entry 不强拥有 group，只保存 `GroupId + generation + Weak/non-owning group handle` 或等价非拥有引用
  - matching 在 registry lock 下解析 group handle；解析失败、generation 不匹配或 group 已 dead 时跳过
  - mark record 强持有首批 target 引用，并有 `target_dead` 标记供 late enqueue fail closed
- `fanotify_mark(ADD)` 支持的首批 event mask 只包括：
  - `FAN_OPEN`
  - `FAN_ACCESS`
  - `FAN_MODIFY`
  - `FAN_CLOSE_WRITE`
  - `FAN_CLOSE_NOWRITE`
- `fanotify_mark(ADD)` 首批必须拒绝的 event mask 包括：
  - permission masks：`FAN_OPEN_PERM`、`FAN_ACCESS_PERM`、`FAN_OPEN_EXEC_PERM`
  - exec-open masks：`FAN_OPEN_EXEC`、`FAN_OPEN_EXEC_PERM`
  - FID/name 或 dirent/name masks：`FAN_CREATE`、`FAN_DELETE`、`FAN_MOVED_FROM`、`FAN_MOVED_TO`、`FAN_RENAME`
  - self/metadata masks：`FAN_DELETE_SELF`、`FAN_MOVE_SELF`、`FAN_ATTRIB`
  - special masks：`FAN_Q_OVERFLOW`、`FAN_FS_ERROR`
- `fanotify_mark(ADD)` 支持的首批 target / modifier / ignore mask 包括：
  - `FAN_MARK_INODE`
  - `FAN_MARK_MOUNT`
  - `FAN_MARK_FILESYSTEM`
  - basic ignored mask
  - `FAN_MARK_ONLYDIR`
  - `FAN_MARK_DONT_FOLLOW`
  - `FAN_EVENT_ON_CHILD`
  - `FAN_ONDIR`
- `fanotify_mark(REMOVE)` 支持移除 mask / mark。
- `fanotify_mark(FLUSH)` 支持按 target class 清除 group marks。
- parser 明确 `FAN_MARK_INODE == 0`：无 mount/filesystem target bit 等价 inode，`FAN_MARK_FLUSH` 遵守同一规则。
- mask parser 明确只接受首批支持的低 32 位 event mask / ignore mask；空 `FAN_MARK_ADD` / `FAN_MARK_REMOVE` mask、未知 mask bit、暂缓 mask bit 或 command / target / modifier 非法组合必须在进入 registry 前返回稳定 errno。
- mount/filesystem mark 需要 `CAP_SYS_ADMIN`。
- permission masks、FID-only masks、dirent/name/self/metadata masks、FS error、evictable mark、`FAN_OPEN_EXEC` 和 `FAN_OPEN_EXEC_PERM` 按暂缓策略拒绝。
- legacy basic ignore mask 支持：
  - `FAN_MARK_IGNORED_MASK` 写入 mark 的独立 `ignored_mask`，不污染普通 event mask。
  - `FAN_MARK_REMOVE | FAN_MARK_IGNORED_MASK` 只移除 ignored mask 中的对应 bits。
  - 未带 `FAN_MARK_IGNORED_SURV_MODIFY` 的 legacy ignored mask 在同一 mark 观察到成功 `FAN_MODIFY` 后清除。
  - 带 `FAN_MARK_IGNORED_SURV_MODIFY` 的 legacy ignored mask 在 modify 后保留。
  - `FAN_MARK_IGNORE` 新语义、mount/filesystem/dir 特殊 errno、ignore mask 与 child/on-dir 的完整 Linux 差异进入 Stage 5 独立 gate；首批遇到 `FAN_MARK_IGNORE` 返回 `EINVAL`。

审计：

- path resolution 遵守 `pathname == NULL`、`dfd`、`DONT_FOLLOW`、`ONLYDIR` 边界。
- mark add/remove/flush 只影响目标 group。
- event mask、ignored mask、target refs、`target_dead` 和 generation 只有一个 owner，target map 与 group cleanup list 不形成双重真相源。
- ignored mask 不被误当成普通 event mask。
- registry 不保存 fd number。
- registry 不强持有 group，不通过 mark entry 延长 group 生命周期。
- target key 相等/hash 规则集中封装，不把 pointer identity 规则散落到 registry callsite。
- path/self/child 匹配不依赖 `PathRef::location_eq()` 的 dentry-only TODO 行为。
- ADD / REMOVE / FLUSH 与后续 matching 使用同一 registry lock 策略序列化。

验证：

- `just build`
- `fanotify04` 的 `ONLYDIR`、`DONT_FOLLOW`、`FLUSH` 基础分支。
- 手工 mark add/remove/flush smoke。
- 手工或源码级验证 close / flush / remove 只删除本 group marks。

退出条件：

- basic mark 操作和错误码稳定。
- LTP 特性探测能把暂缓项归类为 unsupported。
- registry key、target lifecycle、cleanup handles 和锁序进入 RFC，不再作为实现期临时选择。

## 阶段 3：基础 VFS 事件 hook 与 path-fd read

目标：让 open/read/write/close 事件进入 fanotify queue。

前置条件：

- 阶段 2 完成。
- fanotify enqueue API 已封装，VFS 不直接操作 registry 内部。
- fanotify 专用 read-user 提交协议或等价 typed `FileOps` / read syscall dispatch 机制已确定；syscall 层不得通过 downcast 识别 fanotify fd。Stage 1 已经为 current `FileStatusFlags` nonblock read 提供同一 dispatch / context 基础，本阶段只扩展 path-fd metadata 提交。
- task/fd 层 `reserve_event_fd()` / commit / rollback helper 已确定，并把 reserved slot 状态纳入 fd table 单一真相源。
- no-notify guard 形态已确定，且只覆盖 fanotify 内部 event-fd helper；event fd 后续 suppression 使用通用 file/opened-description notification flag。
- queue cap 已在 Stage 1 生效。

fanotify read-user 提交协议：

1. `read(fanotify_fd)` 走 fanotify 专用 read-user path，不复用 “先生成 kernel buffer、再 generic copyout” 的普通 `FileOps::read` 事务。该路径应通过 typed file operation / read dispatch 暴露给 read/readv 系列 syscall，并能观察当前 opened file description status flags；syscall 层不能直接识别 fanotify private state。
2. 在 group lock 下处理 empty blocking/nonblock、buffer-too-small、close/dead wakeup；选择一个完整 event 后从队列移除，普通通知 event 的消费点就是出队点，首批不保留重读。
3. 释放 group lock 后构造 metadata。path-fd event 使用 `NoNotifyGuard` 打开事件对象；对象打开失败时输出 `fd = FAN_NOFD` 并继续 copyout，不 panic，也不把 open failure 变成半读。
4. event fd 安装使用 task/fd 层的 “预留但未发布” 或等价可回滚 helper：`reserve_event_fd()` 返回未发布但独占的 fd slot 和稳定 fd number；commit 只发布已准备好的 file，不分配、不阻塞、不可失败；rollback 在 commit 前幂等释放 slot 和 file。reserved slot 状态必须由 task/fd 层与普通 fd table open-state 一起维护，不能让 fanotify 自己保存并行 fd 分配状态。
5. 一个 metadata record 不能半提交。若本次 read 已经成功提交前面的完整 records，后续 record copyout 失败时返回已提交字节数；未提交 record 的 fd rollback，event 按普通通知消费策略丢弃。若没有任何 record 提交成功，则返回 copyout 错误。首批采用这一简化返回策略，Linux `EFAULT` 细节差异必须在验证记录中标注。
6. Stage 1 synthetic event 固定 `fd = FAN_NOFD`，不进入 path-fd open，不需要 `NoNotifyGuard`。

交付：

- 在 open path 成功后派发 `FAN_OPEN`。
- 在用户可见 opened-fd read/read_at 成功读取后派发 `FAN_ACCESS`；hook 放在 fd/VFS gate 已完成、backend 成功返回之后，不下沉到具体 backend `FileOps::{read,read_at}`，也不把内核内部直接 `File` helper 当作事件源。
- 在用户可见 opened-fd write/write_at/append、truncate 或等价内容修改路径成功后派发 `FAN_MODIFY`；hook 放在 VFS/opened-file 边界，避免每个 backend 重复 fanotify policy。
- 在 opened file description release 中派发 `FAN_CLOSE_NOWRITE` / `FAN_CLOSE_WRITE`：
  - open 成功时在 opened file description 上记录 fanotify close snapshot：`PathRef` / `FanPathKey`、打开时 access mode 是否包含写能力。
  - write/truncate/content modify 成功路径仍记录 `did_modify` 或等价状态，但该状态只服务 `FAN_MODIFY` 和 legacy ignore-mask clearing，不参与 close mask 分类。
  - 最后 release 时，若 snapshot 的 access mode 具备写能力则生成 `FAN_CLOSE_WRITE`，否则生成 `FAN_CLOSE_NOWRITE`；dup/fork 共享 opened file description 时只在最后 release 产生一次 close event。
- 若短期为了 LTP 单 fd 场景使用 fd-close bridge，必须标注为 temporary LTP bridge，不能算完整 release 语义闭合；带 bridge 的实现不能通过 Stage 3 退出条件，只能记录为降级验收或临时得分路径。
- matching 支持：
  - inode mark
  - mount mark
  - filesystem mark
  - target self
  - parent directory + `FAN_EVENT_ON_CHILD`
  - `FAN_ONDIR`
  - basic ignore mask
- 增加 `NoNotifyGuard` 或等价 notify context：
  - `NoNotifyGuard` 只能由 fanotify event-fd helper 创建，只抑制 helper 打开 metadata fd 时产生的 fanotify enqueue。
  - 返回给用户的 event object fd 还必须在 opened file description 上携带 kernel-only 通用 notification suppression 标记，确保该 fd 后续 read/write/close 不反向生成 fanotify event；task/fd 核心不得依赖 fanotify 具体 private type。
  - guard 是 RAII/drop-safe，open 失败、copyout 失败和 rollback 都必须退出构造期 no-notify 状态。
  - event-fd no-notify 标记只属于 fanotify 生成的 event fd，不传播到普通用户 open，不绕过普通 VFS 权限、生命周期或用户可见 I/O 语义。
- read event 时创建 path-fd metadata `fd`：
  - 按 group `event_f_flags` 解析 access/status/fd flags。
  - 通过 fanotify 专用 read-user 提交协议安装到当前 task fd table。
  - metadata copyout 失败后不得留下用户不可见的新 event fd。
  - 对象打开失败时返回 `FAN_NOFD` 或稳定错误策略。
  - fanotify 内部 open 以及返回 event fd 的后续 I/O / close 都不递归生成 fanotify event。

审计：

- hook 不在 VFS mutating lock 内打开 fd 或 copy user buffer。
- internal event fd open 有 no-notify guard。
- event fd 的 opened file description 有 no-notify 标记，且该标记不泄漏到普通 open。
- copyout 失败、event object open 失败和 partial read 不会半成功。
- read/readv 入口不会通过 syscall 层 downcast fanotify private state；fd reservation commit/rollback 由 task/fd helper 统一维护。
- `pread` / `pwrite` / positioned VFS paths 的普通文件事件被覆盖，但对 fanotify group fd 自身的 `read_at` / `write_at` 仍 fail closed，不产生 metadata 或 permission response。
- write/read 失败不生成成功事件。
- close event 来源是 opened file description release；若保留 bridge，注释必须写明 temporary LTP bridge。
- ADD / REMOVE / FLUSH 与 matching snapshot / queue append 的线性化点无歧义。

验证：

- `just build`
- `fanotify01` 非 FID 基础路径。
- `fanotify02` 基础 child open/access/modify/close。
- 手工验证 event fd 可 readlink 到目标对象或至少可读写符合 event flags。
- 手工或源码级验证 metadata copyout 失败不会泄漏 event fd。
- 手工或源码级验证 event fd 后续 read/write/close 不递归产生 fanotify event。

退出条件：

- path-fd 基础通知闭环可用。
- inode/mount/filesystem mark 对基础事件可匹配。
- `FAN_CLOSE_*` 的正式 opened file description release 语义闭合；temporary bridge 只能作为降级记录，不能满足 Stage 3 退出条件。

## 阶段 4：FID/name 边界验证与 dirent hook 草稿

目标：在继续暂缓 FID/name record 的前提下，验证目录项/name 类事件不会伪成功；可选准备内部 dirent hook 草稿，但不作为用户可见正向 ABI。

前置条件：

- 阶段 3 完成。
- dirent hook 输入能表达 parent/name/old/new。

交付：

- 保持 FID/name report flags 继续拒绝，避免向用户态输出不完整 info record。
- 将 `FAN_CREATE`、`FAN_DELETE`、`FAN_MOVED_FROM`、`FAN_MOVED_TO`、`FAN_RENAME` 等依赖 name record 的正向用户可见事件移入 FID/name follow-up。
- `FAN_ATTRIB`、`FAN_DELETE_SELF`、`FAN_MOVE_SELF` 等 metadata/self 事件只有在能证明 path-fd observable ABI 兼容时才可单独提前；否则同样移入 follow-up。
- 可选为 `vfs_touch_at` / `vfs_mkdir_at` / `vfs_unlink_at` / `vfs_rmdir_at` / `vfs_rename_at` 准备内部 hook 输入模型，但 hook 不得向用户态输出假事件。

审计：

- 暂缓 mask / report flag 的 errno 策略稳定，不形成 silent success。
- parent/child/self 输入模型不递归到 FID/name ABI。
- 若提前支持某个 metadata/self event，必须同时更新非目标、接受边界和验收项。

验证：

- `just build`
- 自建或裁剪 probe 校验 FID/name 暂缓项返回稳定 errno。
- stock `fanotify14` 整体归入 FID/name 暂缓，不能作为非 FID 负例直接验收。
- 可选内部 dirent hook smoke 只能验证内核输入模型，不作为用户可见 fanotify ABI 通过证据。

退出条件：

- 不误开启 FID/name ABI。
- dirent/name 类事件的正向支持已有 follow-up gate，或明确继续暂缓。

## 阶段 5：独立 backlog gates

目标：按失败日志选择性提升得分。Stage 5 不是单一 checkpoint；每个候选项必须拆成独立小 gate。

候选交付：

- `FAN_UNLIMITED_QUEUE`、queue sysctl 和更精确 overflow merge/order，在 Stage 1 overflow sentinel 之上争取 `fanotify05` 的剩余分支。
- `FAN_MARK_IGNORE`、inode + mount ignore mask 合并和完整 child/on-dir ignore 规则，争取 `fanotify06` / `fanotify10` 的非 FID 部分。
- `FAN_REPORT_TID`，如果只需 pid/tid 字段切换且不牵涉 FID。
- group / mark limit，争取 `fanotify17` 的非 userns 部分。
- `/proc/<pid>/fdinfo/<fanotifyfd>` 最小 mark 输出，只在能稳定输出真实 group/mark 状态时支持。
- `FAN_OPEN_EXEC`，接 exec loader 的打开事件点。

审计：

- 每个候选项单独评估是否改变不变量。
- 只接受能真实观测的 feature flag。
- 对应 init/mark flag matrix 必须同步更新 accept/reject 策略。
- 失败归类必须区分基础机制缺陷和高阶语义暂缓。

验证：

- `just build`
- 对应 LTP fanotify 子用例。

退出条件：

- 新增 feature 不扩大第一阶段已闭合边界的风险。

## 暂缓清单

以下项目不作为当前实施计划的阶段 gate：

- FID/name records：`fanotify13`、`fanotify15`、`fanotify16`、`fanotify09` FID/name 分支。
- permission events：`fanotify03`、`fanotify07`。
- unprivileged FID listener：`fanotify18`、`fanotify19`。
- pidfd：`fanotify20`、`fanotify21`。
- FS error：`fanotify22`。
- evictable marks：`fanotify23`。

stock LTP 分类必须按具体 helper 路径记录，不能把所有 unsupported errno 都描述成 TCONF：

| 分类 | 用例 / 分支 | 首批期望 |
| --- | --- | --- |
| 第一阶段目标 | `fanotify01`、`fanotify02`、`fanotify04`、`fanotify08`、`fanotify12` 普通 `FAN_OPEN` 分支 | 应给出通过或基础机制缺陷证据。 |
| helper TCONF / unsupported probe | 使用 helper probe 检测 FID/name、permission、pidfd、exec-open 或 unprivileged 能力的分支 | 通过稳定 `EINVAL` / `EPERM` 归类为暂缓。 |
| full-run 预期失败 / TBROK | stock 用例直接 `SAFE_FANOTIFY_INIT()` / `SAFE_FANOTIFY_MARK()` 触发暂缓 flag，例如 `FAN_UNLIMITED_QUEUE`、FID/name setup 或未裁剪的 `fanotify14` | 不作为第一阶段失败修复入口；事务日志记录为暂缓项触发，不扩大首批范围。 |

## 旁路审计清单

实现阶段每个 gate 至少搜索并分类：

- `rg -n "fanotify|fsnotify|FAN_" anemone-kernel anemone-abi`
- `rg -n "after_modified|ModifType|vfs_touch|vfs_mkdir|vfs_unlink|vfs_rmdir|vfs_rename|finish_open|close_fd" anemone-kernel/src/fs anemone-kernel/src/task`
- `rg -n "PollRequest|PollRegisterResult|LatchTrigger|poll:" anemone-kernel/src`
- `rg -n "FIONREAD|fdinfo|pidfd|name_to_handle|open_by_handle" anemone-kernel anemone-abi`

允许保留的旁路必须标注为暂缓或迁移桥，不能成为 silent success。

## 可观测性清单

- group id / fd / task id。
- mark add/remove/flush target 和 mask。
- event enqueue target、mask、matched mark count。
- queue overflow / dropped event count。
- poll register wait id、queue length、trigger count。
- read 输出 event count、bytes、event fd 创建失败原因。
- close teardown group marks count 和 pending queue count。

## 停止边界

如果实现中发现必须支持 FID/name、permission event、pidfd 或 procfs fdinfo 才能修复当前阶段失败，应先停下更新 RFC 或新增 follow-up，而不是在基础通知阶段临时塞入半实现。

如果失败只来自 Linux merge/order 精确性、unprivileged listener、FS error、evictable mark 或 user namespace limit，应记录为暂缓项，不扩大首批范围。
