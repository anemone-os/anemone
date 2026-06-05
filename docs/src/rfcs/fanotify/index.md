# RFC-20260604-fanotify

**状态：** Draft，已提升为公开 RFC 草案
**负责人：** doruche, Codex
**最后更新：** 2026-06-05
**领域：** fs / VFS / syscall ABI / iomux / procfs / LTP
**事务日志：** None；进入实现阶段前建立。
**开放问题：** [fanotify tracking issues](./tracking-issues.md) 当前无 Open Keter / Euclid；强一致或高成本 Linux 语义仍按非目标和 Stage 5 backlog 暂缓。
**下一步：** 完成公开 RFC review；若进入实现，先创建事务日志，再按 [迁移实施计划](./implementation.md) 推进 Stage 0 + Stage 1 合并 gate，并在实现证据中复核 feature gate、module/API 可见性、registry/lifecycle、path-fd read 提交、fd reservation 和 close release 来源。

## 摘要

本 RFC 记录 Anemone fanotify 机制的 staged 实现边界。目标不是一次性复制完整 Linux fsnotify/fanotify，而是先建立能支撑 LTP 基础分数的 path-fd 通知路径：`fanotify_init()`、`fanotify_mark()`、fanotify group fd 的 `read()` / `poll()` / `close`、inode / mount / filesystem mark，以及 `FAN_OPEN`、`FAN_ACCESS`、`FAN_MODIFY`、`FAN_CLOSE` 等基础事件。

FID/name records、pidfd、permission events、完整 `/proc/<pid>/fdinfo`、FS error、evictable marks、exec-open events 和精确 merge/order 语义暂不作为首批闭环。首批必须稳定地区分 unsupported feature 和 invalid input；LTP 结果按具体 helper 路径归类为 TCONF、预期暂缓失败或第一阶段缺陷，不能把所有 unsupported errno 都泛化成 TCONF。

## 背景

LTP 的 fanotify 组占分高，当前内核只有 `SYS_FANOTIFY_INIT` / `SYS_FANOTIFY_MARK` syscall number，尚无 fanotify UAPI 常量、syscall handler、group fd、mark registry 或 VFS 事件派发路径。

现有基础设施已经足够启动 stage-1：

- 匿名 inode 与 `anony_open_with()` 可以承载 fanotify group fd。
- `FileOps` 已有 `read`、`write`、`poll`，typed `PollRequest` / `PollRegisterResult` 和 sched latch 已能支撑可阻塞 readiness。
- `PathRef = Mount + Dentry`、`InodeRef`、`Mount`、`SuperBlock` 足够表达 fanotify mark target。
- `openat`、`File::read*()`、`File::write*()`、metadata syscall 和 VFS dirent primitive 已有集中事件注入点。
- `CredentialSet` 与 `Capability::SYS_ADMIN` 已能表达首批 privileged listener 边界。

详细基础设施评估见 [fanotify 基础设施评估](./backgrounds/infra-assessment-20260604.md)。

## 目标

- 建立 fanotify syscall ABI 边界和 Linux-compatible flag / errno validation。
- 建立 fanotify group fd：`read()`、`poll()`、nonblock read、`FIONREAD` 可选支持、close teardown。
- 建立 mark registry，支持 inode / mount / filesystem mark 的基础匹配。
- 覆盖 path-fd 模式的基础事件：`FAN_OPEN`、`FAN_ACCESS`、`FAN_MODIFY`、`FAN_CLOSE`。
- 支持 `FAN_MARK_ONLYDIR`、`FAN_MARK_DONT_FOLLOW`、`FAN_MARK_FLUSH`、`FAN_EVENT_ON_CHILD`、`FAN_ONDIR` 和 basic ignore mask。
- 对高阶暂缓特性返回稳定 Linux-compatible 错误，避免空成功语义。
- 给 LTP fanotify 用例建立明确分层：首批争取、第二阶段争取、helper TCONF 和 stock 暂缓失败。

## 非目标

- 不实现完整 Linux fsnotify backend、fanotify event merge/order 规则或所有资源上限语义。
- 不在首批支持 FID/name reporting：`FAN_REPORT_FID`、`FAN_REPORT_DIR_FID`、`FAN_REPORT_NAME`、`FAN_REPORT_TARGET_FID`。
- 不在首批支持 pidfd reporting：`FAN_REPORT_PIDFD`。
- 不在首批支持 permission events：`FAN_OPEN_PERM`、`FAN_ACCESS_PERM`、`FAN_OPEN_EXEC_PERM`。
- 不在首批支持完整 `/proc/<pid>/fdinfo/<fanotifyfd>` mark 输出。
- 不在首批支持 `FAN_FS_ERROR`、`FAN_MARK_EVICTABLE`、unprivileged FID-only listener、`FAN_OPEN_EXEC` 或 dirent/name/self/metadata 事件。
- 不通过在 VFS 热路径里散落 Linux ABI struct 来实现功能；Linux ABI 必须停在 syscall / fanotify fd copy 边界。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [tracking issues](./tracking-issues.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [Linux fanotify ABI 与客观语义参考手册](./backgrounds/fanotify-reference-20260531.md)
- [fanotify 基础设施评估](./backgrounds/infra-assessment-20260604.md)
- [LTP fanotify 测例调查报告](./backgrounds/ltp-fanotify-coverage-20260605.md)

## 方案

新增 `fs::fanotify` 子系统模块，拥有 fanotify group、mark registry、event queue、event matching 和 fanotify fd file ops。该模块按 group / event / queue / registry / mark / file / hooks 等内部职责拆分，只有 `mod.rs` facade 暴露窄 API；syscall、VFS、task/fd 和 procfs 不直接访问 registry、queue、mark record 或 group lock。syscall handler 只负责 Linux ABI 参数解析、flag validation、用户指针读取和 fd 安装；fanotify 内部长期状态使用 Anemone 语义类型，例如 `FanMask`、`FanMarkFlags`、`FanGroupMode`、`FanEventKind`、`FanTarget`。

fanotify group fd 使用匿名 inode 创建。group private state 保存 init flags、event fd flags、marks、event queue、poll trigger queue 和 teardown 状态。`read()` 从队列取出事件，path-fd 模式在 read 时为事件对象创建新的 fd；`poll()` 只暴露 queue readiness；event enqueue 在 queue 从空变非空时触发已注册的 latch triggers。

VFS 事件派发通过窄 hook 接入现有集中入口。hook 只接收事件语义、当前 task identity、target `PathRef` 和必要 parent/name 信息；它不在持有 VFS mutating lock 时打开事件 fd 或 copy user buffer。matching 只生成 fanotify queue item，实际 fd 分配和 userspace copy 发生在 group fd read 边界。

首批 privileged path-fd listener 可以支持 inode / mount / filesystem mark。非特权 listener、FID mode、pidfd、permission event、exec-open event、dirent/name/self/metadata event 和 FS error 均先通过 validation gate fail closed；LTP 结果按具体 helper 路径记录为 TCONF 或暂缓失败。

Stage 0 不作为独立用户可见 LTP gate。`fanotify_init()` 公开成功必须和最小 group fd read/poll/close wakeup、nonblock 行为和 bounded queue 同批验收，避免 syscall 成功但事件机制不可用的假进展。

## 接受边界

本 RFC 已提升为公开目录级 RFC 草案，是后续 fanotify 设计 review 和实现准备的 canonical 文档入口。进入实现阶段前必须创建事务级 devlog，并把事务日志与本 RFC 双向链接。

接受本 RFC 意味着 fanotify 可以按 staged feature 推进，第一阶段只需证明 path-fd 通知类机制闭合。以下变化必须回到本 RFC 或新增 follow-up RFC：

- 把 FID/name reporting、pidfd、permission events 或 FS error 提前并入第一阶段验收。
- 改变 fanotify group fd、mark registry、VFS hook 或 event queue 的状态所有权。
- 让 fanotify 内部长期保存 Linux UAPI struct 而不是语义类型。
- 在 VFS mutating lock 内执行 event fd open、用户态 copy 或可能重入 VFS 的长路径操作。
- 对暂缓特性返回成功但不提供真实可观测语义。

## 备选方案

### 完整复制 Linux fsnotify/fanotify

延期。Linux fsnotify 涉及 mark connector、exportfs/file handle、permission wait、pidfd、procfs fdinfo 和复杂 event merge。直接复制会把第一版 fanotify 变成跨 VFS、procfs、task、exec、pidfd 和 fs export 的大工程，风险高于首批 LTP 得分需求。

### 只做 syscall stub

拒绝。`fanotify_init()` 成功但事件机制不可用会让 LTP 从 TCONF 变成后续 BROK/TFAIL，制造假进展。stage-0 可以做 feature gating，但必须紧接 group fd 和基础通知队列。

### 只支持 inode mark

延期作为可能切分，但不是最终第一阶段边界。`FAN_MARK_MOUNT` / `FAN_MARK_FILESYSTEM` 对 LTP fanotify01、03、05、06、10 等用例影响大；现有 `Mount` / `SuperBlock` 基础足够，首批至少应把 target identity 和基本匹配纳入计划。

### 优先实现 permission events

拒绝作为首批。permission events 需要 open/access 路径阻塞、用户态回复、pending permission queue、signal/kill 交互和 close 放行语义；这会拖慢基础通知类闭环。

## 风险

- unsupported feature 返回 errno 不稳定，会让 LTP probe 误分类。控制方式是在阶段 0 固定 flag/mask validation 和暂缓项返回策略。
- event enqueue 持有 VFS lock 后执行长路径操作，可能引入重入或锁反转。控制方式是 hook 只生成 queue item，event fd 创建延迟到 read。
- event fd 创建递归触发 fanotify，导致自激事件。控制方式是 fanotify 内部 open 使用 no-notify guard 或专用 helper。
- `FAN_CLOSE_*` 事件若误接到 fd number close，会在 dup/fork 或 shared file description 下产生假事件。控制方式是正式语义绑定 opened file description release；短期 fd-close bridge 只能标注为 temporary LTP bridge，不能算完整闭合证据。
- 目录项/name 类事件若在继续拒绝 FID/name reporting 时正向输出 path-fd metadata，会形成假 ABI。控制方式是 Stage 4 只做 FID/name 负例验证和内部 hook 草稿，正向 dirent/name 事件移入 follow-up。
- event merge/order 不精确会影响回归类测试。控制方式是首批聚焦基础事件集合，复杂 merge/order 列为第二阶段或暂缓。

## 收口

进入实现后，事务日志需要记录：

- fanotify syscall probe 和 flag validation 结果。
- group fd read/nonblock/poll 验证结果；`FIONREAD` 若实现则记录验证结果。
- inode / mount / filesystem mark 对 open/read/write/close 的最小闭环。
- LTP fanotify01、02、04、08、12 普通 open 分支的通过/失败分类。
- 暂缓项对应 helper TCONF、stock 暂缓失败或稳定 unsupported errno 的证据。

第一阶段完成后，剩余失败应能明确归类为 FID/name、pidfd、permission event、fdinfo、FS error、evictable mark、event merge/order 或 unprivileged listener。
