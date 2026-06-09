# fanotify tracking issues

**状态：** Active
**最后更新：** 2026-06-09
**父 RFC：** [RFC-20260604-fanotify](./index.md)
**来源：** 2026-06-04 system-design review；2026-06-05 software-engineering review；2026-06-05 ioctl-loop / fileops-seek freshness review；2026-06-09 Gate C runtime regression review

本文只跟踪 design review 后确认的 fanotify 草案缺陷、证明缺口、边界冲突或需要回到草案修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。

分级沿用 Anemone review 口径：

- **Keter**：会阻塞后续实现方向或导致核心抽象不可复审，必须修正或明确改边界。
- **Euclid**：值得修正，但通常不阻塞第一版实现。
- **Safe**：记录即可，除非顺手修正。

## Apollyon

- 暂无。

## Keter

### FANOTIFY-041: fd table / task `Drop` 不得触发 group semantic teardown

**状态：** Keter

**发现证据：** D4 完成后的 `fanotify07` runtime smoke 中，permission event probe 按当前 RFC
暂缓边界返回 `EINVAL` 并被 LTP 归类为 `TCONF`，但 testcase 退出后 panic：
`fanotify/registry.rs:218: Mutex cannot be locked when interrupts are disabled`。触发链路是
fanotify group fd opened-description final-release 进入 `mark_dead()`，而 final-release 可能由
task / fd table 的 deferred drop 或 scheduler/trap cleanup 间接触发，此时 interrupts 可能已关闭。

**RFC 修正落点：**

- [不变量需求](./invariants.md) 现在明确：group semantic teardown 必须由 task/fd 显式
  close / exit / fd-table replacement 生命周期路径触发；`Drop`、memory last-drop 和 deferred
  task dispose 不得执行 fanotify registry mutation、waiter wakeup 或其它系统资源释放。
- [迁移实施计划](./implementation.md) 的 Stage 1 现在要求 opened-description final-release hook
  在可睡眠、interrupts-enabled 的显式生命周期路径运行；`FilesState::drop()` / task `Drop`
  只能释放内存或断言显式清理遗漏。

**原问题：** RFC 已经区分 semantic close 与 memory last-drop，但仍允许实现者把
opened-description final-release 的最后兜底留给 fd table / task 析构路径。析构路径可能出现在
deferred task dispose、scheduler loop 或 interrupt-return cleanup 中，不能拿普通 `Mutex`、触发
waiter 或修改全局 registry。若继续让 `Drop` 补做系统资源释放，fanotify group close、mark
registry cleanup 和后续 `FAN_CLOSE_*` release 都会继承同一个不可复审的上下文风险。

**实施修复要求：** task/fd 层必须在显式 close、task exit、fd-table replacement / unshare 等路径
drain published fd refs 并运行 opened-description final-release；`FilesState::drop()` 不得再调用
`release_description_ref()`，只能断言没有 published / reserved slot。fanotify final-release 入口
应断言当前可睡眠 / interrupts enabled，以防后续回归。修复后 `fanotify07` 可以继续 TCONF，但不
得 panic。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

### FANOTIFY-001A: blocking `read(fanotify_fd)` 与首个 group fd gate 未闭合

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案明确 Stage 0 不作为独立用户可见 LTP gate；`fanotify_init()` 公开成功必须和最小 group fd read/poll/close wakeup、nonblock 行为和 bounded queue 同批验收。
- [不变量需求](./invariants.md) 的闭合条件和线性化点明确：empty blocking read 必须等待并可被 enqueue / close / dead-group 唤醒；empty nonblock read 返回 `EAGAIN`。
- [迁移实施计划](./implementation.md) 的 Stage 1 明确：Stage 1 synthetic event 使用 `fd = FAN_NOFD`，不创建 path-fd event object。

**原问题：** 原草案阶段 1 允许 empty blocking read “等待或返回 `EAGAIN`”。一旦 `fanotify_init()` 成功创建 blocking group fd，empty read 返回 `EAGAIN` 就是伪 Linux 语义；`EAGAIN` 只能属于 nonblock fd。

### FANOTIFY-001B: path-fd event read 提交事务未闭合

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 `read(fanotify_fd)` 线性化点固定为 fanotify 专用 read-user 提交协议：event 出队是普通通知 event 的消费点，fd 预留/安装必须可回滚，copyout 全部成功后才 commit。
- [迁移实施计划](./implementation.md) 的 Stage 3 写入 read-user 算法：不复用 generic kernel-buffer read；copyout 失败 rollback 未提交 fd；对象打开失败输出 `FAN_NOFD`；一个 metadata record 不能半提交。

**原问题：** path-fd event read 会在 `read()` 时把事件对象 fd 安装到当前 task fd table。若 fd 已安装、event 已出队，但 copyout 失败，原草案没有规定新 fd、event、返回值如何处理。

### FANOTIFY-002A: group fd lifecycle / teardown 未绑定 shared group state

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的闭合条件、身份模型和 group semantic teardown 线性化点明确：最后一个用户可见 descriptor / opened-description ref 关闭时进入 closing/dead；memory last-drop 只负责最终释放。
- [迁移实施计划](./implementation.md) 的 Stage 1 / Stage 2 明确 group state 保存 queue、poll triggers、dead/closing 状态、group-owned mark handles、`GroupId` / generation 和 semantic close release hook / table-ref 计数。
- [不变量需求](./invariants.md) 的 teardown 规则要求 queue 内容搬出锁外释放，并唤醒 blocking read / poll waiters。

**原问题：** 原草案把 close teardown 写成 group state Drop / close teardown，但现有 `close_fd()` 只是从 fd table 回收引用，不能代表 anonymous opened file description 的最后释放；dup/fork 后多个 fd 可能共享同一个 file/private state。

### FANOTIFY-002B: `FAN_CLOSE_*` release 事件来源未闭合

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的风险明确：`FAN_CLOSE_*` 正式语义绑定 opened file description release；fd-close bridge 只能是 temporary LTP bridge。
- [不变量需求](./invariants.md) 的状态所有权明确：每个被监控 opened file description 保存 fanotify close snapshot，dup/fork 共享时只在最后 release 产生一次 close event。
- [迁移实施计划](./implementation.md) 的 Stage 3 明确 snapshot 字段：`PathRef` / `FanPathKey` 和打开时 access mode 是否包含写能力；release hook 基于写打开能力选择 `FAN_CLOSE_WRITE` 或 `FAN_CLOSE_NOWRITE`。

**原问题：** `FAN_CLOSE_NOWRITE` / `FAN_CLOSE_WRITE` 的语义来源必须是被监控对象的 opened file description release，而不是 fanotify group fd 自己的 close，也不是任意 fd number close。

### FANOTIFY-003: event queue 缺少首批硬上限

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的闭合条件要求首个真实 VFS enqueue 前必须有固定 cap 和 overflow sentinel。
- [迁移实施计划](./implementation.md) 的 Stage 1 固定默认 `DEFAULT_MAX_EVENTS = 16384`，group state 保存 `max_events`、`overflow_queued`、`dropped_events`。
- [迁移实施计划](./implementation.md) 明确队满策略：若没有 overflow sentinel，排入单个 `FAN_Q_OVERFLOW` / `FAN_NOFD` event；若 sentinel 已排队，丢弃新事件并计数。

**原问题：** queue item 可能持有 `PathRef`，会延长 `Mount` / `Dentry` / inode 生命周期；只要开始真实 enqueue path-fd event，无界队列就是内核资源和引用生命周期风险。

### FANOTIFY-004: mark target identity 未闭合为 registry key

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的身份模型明确 `FanTargetKey::{Inode, Mount, SuperBlock}` 集中定义相等和 hash；首批可封装 pointer identity 或对象内 stable id，但 callsite 不得比较裸指针。
- [迁移实施计划](./implementation.md) 的 Stage 2 定义 `FanPathKey { mount, dentry }`，path/self/child 匹配不得依赖 `PathRef::location_eq()` 的 dentry-only TODO 行为。
- [迁移实施计划](./implementation.md) 的 Stage 2 给出 `marks_by_target`、`mark_handles` 和 `MarkHandle` 数据关系。

**原问题：** 原草案写 inode / mount / filesystem mark 使用 `InodeRef`、`Arc<Mount>`、`Arc<SuperBlock>` identity，但没有定义可复审的 key 构造、相等和 hash 规则。

### FANOTIFY-005: mark 对目标生命周期和 umount 的副作用未定义

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的身份模型明确首批 lifecycle 采用强引用 + pre-unmount flush / mark-dead。
- [迁移实施计划](./implementation.md) 的 Stage 2 前置条件明确：mark 和 queued event 可以 pin 目标，但完整 umount/filesystem kill 兼容必须等 VFS 接入 pre-unmount 清理 hook 后才能宣称。
- [不变量需求](./invariants.md) 的 matching / enqueue 规则要求 late enqueue 观察到 dead target 或 dead group 时 fail closed。

**原问题：** registry 若强持有 `InodeRef`、`Arc<Mount>` 或 `Arc<SuperBlock>`，会影响 inode eviction、umount 和 filesystem kill；若只弱引用，又必须定义 late matching 和 cleanup 语义。原草案没有选择策略。

### FANOTIFY-006: ADD / REMOVE / FLUSH 与 matching 线性化不够精确

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的线性化点明确 matching snapshot 是 event 的线性化点，并与 ADD / REMOVE / FLUSH 由同一 registry lock 序列化。
- [不变量需求](./invariants.md) 的锁序固定为 `registry -> group`：registry lock 下取得 matching snapshot，按固定 group id 顺序进入 group lock，最后重查 group dead、target dead、queue cap 和 overflow sentinel。
- group teardown 使用同一状态边界标记 dead、移除 marks、detach waiters，并把 queue drop 放到锁外释放。

**原问题：** 原草案定义了 mark add/remove/flush 和 enqueue 的线性化点，但没有说明 event matching snapshot 与 queue append 是否在同一 registry 事务内，导致 “remove 已返回后仍 enqueue” 的歧义。

### FANOTIFY-007: Stage 4 与 FID/name 暂缓边界冲突

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的非目标和风险明确 FID/name reporting 首批暂缓，继续拒绝时不得正向输出 dirent/name 类假事件。
- [迁移实施计划](./implementation.md) 的 Stage 4 改为 “FID/name 边界验证与 dirent hook 草稿”：只做负例验证和内部 hook 输入模型，不作为用户可见正向 ABI。
- [迁移实施计划](./implementation.md) 把 `FAN_CREATE`、`FAN_DELETE`、`FAN_MOVED_FROM`、`FAN_MOVED_TO`、`FAN_RENAME` 等依赖 name record 的正向用户可见事件移入 FID/name follow-up。

**原问题：** 原草案 Stage 4 计划正向支持目录项/name 类事件，但同时继续拒绝 FID/name reporting，容易形成不可兼容的空成功或假事件。

### FANOTIFY-008: Stage 0 不是独立用户可见 checkpoint

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案明确 Stage 0 不作为独立用户可见 LTP gate。
- [不变量需求](./invariants.md) 的 `fanotify_init()` 线性化点明确：公开成功 gate 必须同时包含最小 group fd read/poll/close wakeup 和 bounded queue 行为。
- [迁移实施计划](./implementation.md) 的 Stage 0 标为内部 checkpoint；第一个公开验收 gate 是 Stage 0 + Stage 1。

**原问题：** 原草案 Stage 0 让 `fanotify_init()` 成功返回 group fd，但 group fd read/poll/queue 和 mark registry 到 Stage 1/2 才出现，会制造 “syscall 成功但事件机制不可用” 的假进展。

### FANOTIFY-009: stage-0 feature gate / errno 矩阵不完整

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 0 新增 init flag matrix，逐项定义 `FAN_CLASS_*`、`FAN_CLOEXEC`、`FAN_NONBLOCK`、`event_f_flags`、FID/name、pidfd、tid、unlimited queue/marks、audit、unprivileged 和 unknown bits 的 accept/reject 策略。
- [不变量需求](./invariants.md) 的 unsupported feature 边界同步明确：`FAN_REPORT_TID`、`FAN_UNLIMITED_QUEUE`、`FAN_UNLIMITED_MARKS`、`FAN_ENABLE_AUDIT` 首批默认 `EINVAL`，不得忽略成功。
- permission class gate 继续保留：`FAN_CLASS_CONTENT/PRE_CONTENT` 可创建 group，但 permission mask 在 `fanotify_mark()` 返回 `EINVAL`。

**原问题：** 原草案没有完整矩阵说明 `FAN_REPORT_TID`、`FAN_UNLIMITED_QUEUE`、`FAN_UNLIMITED_MARKS` 等 flag 的 accept/reject 策略、errno 和 LTP 分类。

### FANOTIFY-010: `FAN_NONBLOCK` 不应固化成 group 私有状态

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的迁移原则和 Stage 0 matrix 明确：`FAN_NONBLOCK` 只设置 group fd 初始 file status flags；`read()` 每次读取当前 flags。
- [不变量需求](./invariants.md) 的 `fanotify_init()` / `read()` 线性化点把 nonblock read 纳入首个公开 gate。

**原问题：** `FAN_NONBLOCK` 应设置 group fd 的初始 file status flag；实际 read 行为应读取当前 `FileStatusFlags`，使 `F_SETFL(O_NONBLOCK)` 后续变更有效。

### FANOTIFY-011: no-notify guard 需要模型化

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的锁序与生命周期规则定义两层 no-notify 模型：构造期 `NoNotifyGuard` 抑制 metadata fd open 自激，返回给用户的 event fd 还带 kernel-only no-notify 标记。
- [迁移实施计划](./implementation.md) 的 Stage 3 将构造期 guard 和 event-fd no-notify 标记作为交付项，并明确它们不绕过普通 VFS 权限、生命周期或用户可见 I/O 语义。

**原问题：** 草案禁止 fanotify 内部 open 递归生成 fanotify event，但没有定义 guard 形态。

### FANOTIFY-012: `FAN_MARK_INODE == 0` 解析规则需显式写入

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 2 parser 规则明确：无 mount/filesystem target bit 等价 inode，`FAN_MARK_FLUSH` 遵守同一规则。

**原问题：** Linux UAPI 中 inode mark 类型是零值，parser 若不显式写入会把 “未设置 target bit” 误判为缺失 target。

### FANOTIFY-013: basic ignore mask 边界需拆清

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的迁移原则和 Stage 2 明确：首批 legacy basic ignore mask 只覆盖 `FAN_MARK_IGNORED_MASK` / `FAN_MARK_IGNORED_SURV_MODIFY` 的 add/remove/modify-survive 语义。
- [迁移实施计划](./implementation.md) 的 Stage 2 明确 `FAN_MARK_IGNORE` 新语义、mount/filesystem/dir 特殊 errno、ignore mask 与 child/on-dir 的完整 Linux 差异进入 Stage 5 独立 gate；首批遇到 `FAN_MARK_IGNORE` 返回 `EINVAL`。
- [不变量需求](./invariants.md) 的 unsupported feature 边界同步把 `FAN_MARK_IGNORE` 列为首批默认 `EINVAL`。

**原问题：** “basic ignored mask” 没有拆清是否覆盖 modify 后清除、survive modify、ignored mask remove、`FAN_MARK_IGNORE` 与 legacy ignored mask 的复杂差异。

### FANOTIFY-014: `fanotify14` stock LTP 不能作为非 FID 负例直接验收

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 4 验证明确：使用自建或裁剪 probe 校验 FID/name 暂缓项返回稳定 errno。
- [迁移实施计划](./implementation.md) 明确 stock `fanotify14` 整体归入 FID/name 暂缓，不能作为非 FID 负例直接验收。

**原问题：** stock `fanotify14` setup 可能先要求 `FAN_REPORT_FID`。若 FID 被阶段 0 拒绝，整案会 TCONF，不能证明非 FID 负例。

### FANOTIFY-015: Stage 1 synthetic event 验收应避免提前拉入 Stage 3

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 1 read 规则、审计项和 read-user 协议均明确 synthetic event 使用 `fd = FAN_NOFD`。
- [不变量需求](./invariants.md) 的 read 线性化点明确 Stage 1 synthetic event 不触发 path-fd event fd 创建。

**原问题：** Stage 1 若需要手工注入事件验证 read/poll，必须避免提前要求 event object fd open 和 no-notify helper。

### FANOTIFY-016: `FIONREAD` 必做/可选口径不一致

**状态：** Neutralized

**修复落点：**

- RFC 和 implementation 已统一为 `FIONREAD` 可选；若实现则验证，未实现时不作为 Stage 1 验收项。

### FANOTIFY-017: Stage 5 应拆成独立 backlog gate

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 5 已改成独立 backlog gates。
- queue cap 与 overflow sentinel 前移到 Stage 1；`FAN_UNLIMITED_QUEUE`、queue sysctl 和更精确 overflow merge/order 保留为后续独立 gate。

### FANOTIFY-018: draft/canonical 文字口径不一致

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的文档索引已改为公开 RFC 口径，不再把私有草案描述成 canonical。

### FANOTIFY-019: `index.md` 的开放问题字段需要更新

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的开放问题字段已改为指向本文。

### FANOTIFY-020: temporary fd-close bridge 不能满足 `FAN_CLOSE_*` 阶段退出条件

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的禁止退化项明确：不能用 fd table slot close、fanotify group fd close 或 private-state drop bridge 冒充被监控 opened file description release，并据此宣称 Stage 3 闭合。
- [不变量需求](./invariants.md) 的完成标准明确：`FAN_CLOSE_*` 必须有 opened file description release source 闭合证据；temporary fd-close bridge 只能作为降级实现记录。
- [迁移实施计划](./implementation.md) 的 Stage 3 退出条件改为正式 release 语义闭合；temporary bridge 不能满足 Stage 3 退出条件。

**原问题：** 原实施计划虽然说 bridge 不能算完整闭合证据，但 Stage 3 退出条件仍允许 “temporary bridge 已明确降级” 和正式 release 语义并列，容易让实现阶段把 fd-close bridge 当作第一阶段闭合。

### FANOTIFY-021: permission class gate 需要同时关闭 response write 假语义

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的状态所有权明确：`write()` 只有在 permission event gate 引入 pending response queue 后才解释 permission response ABI；首批通知类实现必须返回稳定 unsupported / invalid errno。
- [迁移实施计划](./implementation.md) 的 Stage 0 matrix 明确：`FAN_CLASS_CONTENT` / `FAN_CLASS_PRE_CONTENT` 只创建通知类 group，不创建 pending permission queue；permission response `write()` 在 follow-up 前返回稳定 unsupported / invalid errno。

**原问题：** 原草案允许 content / pre-content group creation 以避免 permission 用例在 init 处 TBROK，但没有同步规定 group fd `write()` 行为。若 `write()` 静默成功或解释无 pending event 的 response，会形成 permission event 假 ABI。

### FANOTIFY-022: mark mask parser 必须在 registry 前拒绝未知和暂缓 bits

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 2 parser 规则明确：只接受首批支持的低 32 位 event mask / ignore mask；空 `FAN_MARK_ADD` / `FAN_MARK_REMOVE` mask、未知 mask bit、暂缓 mask bit 或非法 command / target / modifier 组合必须在进入 registry 前返回稳定 errno。
- [不变量需求](./invariants.md) 的 unsupported feature 边界仍要求暂缓 mask 不能伪成功。

**原问题：** 原草案写了 permission / FID-only / FS error / evictable mark 按暂缓策略拒绝，但没有把 unknown mask bit、空 add/remove mask 和 command/target/modifier 非法组合的拒绝点固定在 registry 前。实现若先创建或更新 mark 再发现 mask 不支持，会污染 registry 状态并破坏失败路径可复审性。

### FANOTIFY-023: event fd no-notify 不能只覆盖构造期 open

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 no-notify 模型改为两层：构造期 `NoNotifyGuard` 抑制 metadata fd open 自激；返回给用户的 event object fd 还必须在 opened file description 上携带 kernel-only no-notify 标记。
- [迁移实施计划](./implementation.md) 的 Stage 3 交付和审计要求 event fd 后续 read/write/close 不反向生成 fanotify event，且标记不得泄漏到普通用户 open。

**原问题：** 原草案只要求 fanotify 内部 open 有 guard，同时禁止 guard 泄漏到普通用户 open/read/write/close。path-fd read 返回给用户的 event fd 后续 I/O / close 若重新进入 fanotify hook，会形成自激队列；但完全禁止 fd 上的持久 no-notify 标记又会挡住正确模型。

### FANOTIFY-024: path-fd read 的 fd reserve / commit helper 缺少不可失败契约

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 task/fd 所有权明确：fd reservation 独占未发布 slot，commit 前不能被其他路径观察或复用，commit 不分配、不阻塞、不可失败，rollback 在 commit 前幂等释放 slot 和 file。
- [迁移实施计划](./implementation.md) 的 read-user 协议同步写入 `reserve_event_fd()` 契约。

**原问题：** 原草案要求 “预留但未发布，copyout 成功后 commit”，但没有说明 reserved fd slot 在 commit / rollback 前不可被其他路径分配或关闭，也没有说明 commit 在用户已经看到 fd number 后必须不可失败。

### FANOTIFY-025: `FAN_CLOSE_WRITE` 分类不能依赖实际写入状态

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 close snapshot 改为记录 opened file description 的对象和打开时 access mode 是否包含写能力；后续 `did_modify` 只服务 `FAN_MODIFY` 和 legacy ignore-mask clearing。
- [迁移实施计划](./implementation.md) 的 Stage 3 close event 规则明确：最后 release 时按 snapshot 的写打开能力选择 `FAN_CLOSE_WRITE` / `FAN_CLOSE_NOWRITE`。

**原问题：** 原草案把 “后续成功内容修改状态” 纳入 close snapshot，容易把 `FAN_CLOSE_WRITE` 误实现成实际写过才触发。基础 LTP 语义按 opened file description 的写打开能力分类。

### FANOTIFY-026: `FAN_OPEN_EXEC` 首批 accept/reject 边界不明确

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 unsupported feature 边界新增：`FAN_OPEN_EXEC` / `FAN_OPEN_EXEC_PERM` 在 exec hook gate 前默认 `EINVAL`，不得被 basic event mask parser 接受。
- [迁移实施计划](./implementation.md) 的 Stage 2 明确首批 event mask 仅包含 `FAN_OPEN`、`FAN_ACCESS`、`FAN_MODIFY`、`FAN_CLOSE_WRITE`、`FAN_CLOSE_NOWRITE`；`FAN_OPEN_EXEC` 和 `FAN_OPEN_EXEC_PERM` 进入暂缓拒绝项。

**原问题：** RFC 把完整 `FAN_OPEN_EXEC` 列为非目标，Stage 5 又把它列为 backlog，但 Stage 2 只写 “basic event mask”，可能让实现者在没有 exec-loader hook 时接受 `FAN_OPEN_EXEC`，形成 unsupported feature 伪成功。

### FANOTIFY-027: 多 record copyout 失败策略需要标注 Linux 差异

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 和 [迁移实施计划](./implementation.md) 的 read-user 协议明确：首批采用 “已提交完整 records 返回已提交字节数；未提交 record rollback；无提交时返回 copyout 错误” 的简化策略，并要求在验证中记录 Linux `EFAULT` 细节差异。

**原问题：** 原草案规定多 record partial copyout 的返回策略，但没有说明这是首批简化选择还是完整 Linux 行为，容易在 ABI 验收时把兼容性差异误归类为实现 bug。

### FANOTIFY-028: 首批 mark event mask accept/reject 矩阵不完整

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 2 明确首批接受的 event mask 仅为 `FAN_OPEN`、`FAN_ACCESS`、`FAN_MODIFY`、`FAN_CLOSE_WRITE`、`FAN_CLOSE_NOWRITE`。
- [迁移实施计划](./implementation.md) 的 Stage 2 同步列出首批必须拒绝的 permission、exec-open、dirent/name、self/metadata 和 special masks。
- [不变量需求](./invariants.md) 的 unsupported feature 边界同步要求 dirent/name/self/metadata masks 在对应 gate 前默认 `EINVAL`，不得进入 registry。

**原问题：** 原草案只写 “basic event mask”，但 Stage 3 实际只承诺 open/access/modify/close，Stage 4 又把目录项/name 事件移入 follow-up。实现者无法判断 `FAN_ATTRIB`、`FAN_DELETE_SELF`、`FAN_MOVE_SELF`、`FAN_CREATE`、`FAN_DELETE`、`FAN_MOVED_FROM`、`FAN_MOVED_TO`、`FAN_RENAME` 等公开 mask 是应拒绝还是可进入 registry。

### FANOTIFY-029: registry entry 不能强持有 group

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的身份模型明确：registry 不得强拥有 group；mark entry 只能保存 `GroupId + generation + Weak/non-owning group handle` 或等价非拥有引用。
- [迁移实施计划](./implementation.md) 的 Stage 2 数据结构同步写入 group generation、non-owning group handle 和 matching 解析失败 / generation 不匹配 / dead group 跳过规则。

**原问题：** 原草案同时要求 group teardown 在 shared state 最后 drop 发生，又要求 registry 保存 mark 并供 matching 找到 group。若 registry 直接持有 `Arc<FanGroup>`，group 永远不会 last drop；若只保存 id，又没有 generation / weak 解析规则。

### FANOTIFY-030: semantic close wakeup 不能依赖 group memory last-drop

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 区分 semantic close 和 memory release：最后一个用户可见 descriptor / opened-description ref 关闭时标记 closing/dead、移除 marks 并唤醒 waiters；内存释放可等 in-flight syscall transient refs 归零。
- [迁移实施计划](./implementation.md) 的 Stage 1 要求 fd/task 层 release hook、table-ref 计数或等价机制触发 semantic close teardown；不得依赖 memory last-drop 唤醒当前正在阻塞且持有 transient ref 的 read。

**原问题：** 阻塞中的 `read()` 自己会持有 file/group transient ref。若 close wakeup 只发生在 shared group state memory last-drop，那么该 read 会阻止 last-drop，导致 Stage 0 + Stage 1 要求的 close/dead 唤醒无法成立。

### FANOTIFY-031: `fs::fanotify` 目录组织和 API facade 需要明确

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案明确 `fs::fanotify` 是 owner module，并固定 `mod.rs`、`types.rs`、`api/{mod,init,mark}.rs`、`group.rs`、`file.rs`、`event.rs`、`queue.rs`、`registry.rs`、`mark.rs`、`hooks.rs` 文件骨架。
- [迁移实施计划](./implementation.md) 的迁移原则、Stage 0 和 Stage 1 前置条件要求 fanotify syscall API 位于 `fs/fanotify/api/`，syscall dispatch / VFS / task-fd 层只通过 typed facade 交互，不能访问 registry、queue、mark record、group lock 或 file private state。
- [不变量需求](./invariants.md) 的状态所有权写入具体文件职责边界，禁止模块外 downcast fanotify private state 或访问内部 storage。

**原问题：** 原草案只写 “新增 `fs::fanotify` 或等价模块”，没有把目录组织、owner module、public facade 和内部 storage 可见性固定下来。实现者可能把 registry helper、group private state 或 file ops cast 分散到 syscall、VFS hook、task/fd 或 procfs 层。

### FANOTIFY-032: mark registry 与 group cleanup list 可能形成双重真相源

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 1 group state 改为只保存 group-owned `MarkHandle`，不得保存独立 `FanMark` 副本。
- [迁移实施计划](./implementation.md) 的 Stage 2 明确 `FanMarkRecord` 只由 registry arena / slot map 或等价 owner 持有；target map 和 group cleanup list 只能保存 `MarkHandle`。
- [不变量需求](./invariants.md) 的身份模型和禁止退化项禁止复制 event mask、ignored mask、target refs、`target_dead` 或 generation。

**原问题：** 原计划同时写 `marks_by_target`、group-owned cleanup handles 和 group state 保存 marks，容易把 mark mask / ignored mask / target lifecycle 存成多份。后续 ADD / REMOVE / FLUSH、ignore mask clearing 和 target-dead 更新会变成双重真相源。

### FANOTIFY-033: fd reservation / release 不能由 fanotify 维护并行 open-state

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的迁移原则和 Stage 1 前置条件明确 fd reservation / commit / rollback 和 opened-description release 由 task/fd 层拥有。
- [迁移实施计划](./implementation.md) 的 Stage 3 read-user 协议要求 reserved slot 状态纳入 task/fd 层 fd table 单一真相源。
- [不变量需求](./invariants.md) 的 task/fd 所有权禁止 fanotify 保存用户 fd number 或维护与 fd table 并行的 descriptor/open-state 真相源。

**原问题：** 当前 fd 层本身已有 bitmap / fd vector 双重真相源风险；如果 fanotify 再保存 reserved fd、descriptor count 或用户 fd number，会放大 fd-table 状态分裂，尤其影响 copyout rollback、close_range、dup/fork 和 semantic teardown。

### FANOTIFY-034: fanotify read-user 不能通过 syscall 层 downcast private state

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 3 前置条件和 read-user 协议要求 fanotify 专用 read path 通过 typed file operation / read dispatch 暴露给 read/readv 系列 syscall。
- [不变量需求](./invariants.md) 的状态所有权要求 syscall 层只能调用 fd/file 层 typed operation 或 fanotify syscall facade，不能通过 `AnyOpaque` downcast 判断 fd 类型。

**原问题：** path-fd read 需要绕开普通 “kernel buffer -> generic copyout” 流程，但若直接在 `sys_read` / `sys_readv` 中识别 fanotify private state，会把 fanotify policy 泄漏到 syscall 层，也会让 read/readv/pread 差异难以复审。

### FANOTIFY-035: event-fd no-notify 标记不能污染 task/fd 核心类型

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的迁移原则和 Stage 3 交付改为通用 file/opened-description notification suppression 标记。
- [不变量需求](./invariants.md) 的 no-notify 模型和禁止退化项要求 task/fd 核心不得依赖 fanotify concrete private type。

**原问题：** 原草案使用 `FanEventFdNoNotify` 作为示例名称，容易被实现成 task/fd 层里的 fanotify-specific 字段或类型判断。正确方向应是通用 notification suppression 能力，由 fanotify 创建 event fd 时设置，VFS hook 读取，不让 fd 核心反向依赖 fanotify。

### FANOTIFY-036: fanotify ABI 组织需要固定到 `anemone_abi::fs::linux::fanotify`

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的 Stage 0 交付明确 fanotify 常量和 `repr(C)` ABI struct 放入 `anemone_abi::fs::linux::fanotify`。
- [迁移实施计划](./implementation.md) 的 Stage 0 同时要求 kernel syscall API 模块封装 ABI parser、errno matrix 和 feature gate，fanotify 内部长期状态继续使用语义类型。

**原问题：** `anemone-abi/src/fs.rs` 已有组织整理压力。若 fanotify 常量、metadata struct、response struct 和 parser 分散在 open/ioctl/syscall 边界附近，后续 FID/name、pidfd、permission response 和 fdinfo gate 会扩大 ABI 表示泄漏面。

### FANOTIFY-037: Stage 1 nonblock read 不能依赖 fanotify 私有 flags 副本

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的迁移原则、Stage 0 matrix 和 Stage 1 前置条件明确：fanotify group fd read 必须观察当前 opened file description `FileStatusFlags`，使 `F_SETFL(O_NONBLOCK)` 后续变更生效。
- [迁移实施计划](./implementation.md) 的 Stage 1 审计和验证要求覆盖 `FAN_NONBLOCK`、`F_SETFL(O_NONBLOCK)`、清除 `O_NONBLOCK` 后恢复 blocking 的行为；类似 pipe 的 private nonblock mirror 只能作为 temporary compatibility bridge，不能作为合并 gate 闭合证据。
- [不变量需求](./invariants.md) 的闭合条件明确：group fd nonblock read 不能固化到 fanotify private state，必须读取当前 opened file description status flags。

**原问题：** Stage 0 + Stage 1 已要求 `fanotify_init(FAN_NONBLOCK)` 和后续 `F_SETFL(O_NONBLOCK)` 影响 empty read，但当前普通 `FileOps::read` 只接收 `&File` / cursor / buffer，看不到 `FileDesc` 上的 shared `FileStatusFlags`。若实现只在 fanotify group state 保存 init-time nonblock，`F_SETFL` 后续变更会失效，首个公开 gate 的 nonblock 语义不闭合。

### FANOTIFY-038: fileops-seek 后 group fd 完整 vtable 行为需要固定

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的背景和方案刷新当前 `FileOps` 事实：`read_at`、`write_at`、`seek`、`read_dir`、`poll`、`ioctl` 都是 fanotify group fd 必须提供的 vtable 面。
- [迁移实施计划](./implementation.md) 的 Stage 1 交付明确：group fd `seek`、`read_at`、`write_at`、`read_dir` fail closed；`ioctl` 只处理可选 group fd 命令，未知命令返回 `UnsupportedIoctl` / `ENOTTY`。
- [不变量需求](./invariants.md) 的闭合条件和状态所有权明确：`lseek` / `pread` / `pwrite` 不得消费 fanotify queue、创建 metadata fd 或绕过 read-user 提交协议。

**原问题：** fanotify RFC 形成后，fileops-seek-char-ioctl 已把 `FileOps::seek`、`read_at`、`write_at` 和 `ioctl` 变成当前 vtable 事实。原 fanotify 文本只规范 `read()` / `write()` / `poll()`，没有说明 fanotify group fd 是 stream-like object，也没有阻止 `pread` / `pwrite` 误进入 metadata read 或 permission response path。

### FANOTIFY-039: `FIONREAD` 不能再通过 `sys_ioctl` fanotify 特判实现

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的背景明确：`sys_ioctl()` 已经通过 `IoctlCtx` 分发到 opened file 的 `FileOps::ioctl`；fanotify `FIONREAD` 若支持，必须在 group fd file ops 内处理。
- [迁移实施计划](./implementation.md) 的 Stage 1 明确：group fd `FIONREAD` 可选，若实现必须走 `FileOps::ioctl`，未知命令返回 `UnsupportedIoctl` / `ENOTTY`。
- [fanotify 基础设施评估](./backgrounds/infra-assessment-20260604.md) 已加审查后注记，说明旧的 `sys_ioctl` fanotify private-state 探测选项已经废弃。

**原问题：** 旧背景材料写过 `sys_ioctl(FIONREAD)` 当时只识别 pipe，并列出在 `sys_ioctl` 内探测 fanotify private state 的短期选项。ioctl-loop 已经建立 `sys_ioctl -> FileOps::ioctl` 的统一边界，继续沿用旧选项会把 fanotify private state 泄漏到 syscall 层。

### FANOTIFY-040: 基础 VFS 事件 hook 必须覆盖 positioned I/O

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的背景刷新事件注入点：`File::{read,read_at}`、`File::{write,write_at,append}` 和 metadata syscall 都属于首批基础事件候选边界。
- [迁移实施计划](./implementation.md) 的 Stage 3 交付明确：`FAN_ACCESS` 覆盖顺序 read 和 positioned read 成功路径；`FAN_MODIFY` 覆盖顺序 write、positioned write、append、truncate 或等价内容修改成功路径。
- [不变量需求](./invariants.md) 的 VFS/opened-file 边界明确：hook 放在 fd/VFS gate 后、backend 成功返回后的统一边界，不下沉到具体 `FileOps::{read_at,write_at}` backend。

**原问题：** fileops-seek-char-ioctl 完成后，`read_at` / `write_at` 是独立 file-op 路径，不再通过普通 `read` / `write` 的 dummy cursor wrapper。原 Stage 3 只写 “read 成功读取” 和 “write/truncate 成功修改” 容易让实现漏掉 `pread` / `pwrite`，导致 positioned I/O 不产生基础 fanotify 事件。
