# mount tree legacy API tracking issues

**状态：** Closed
**最后更新：** 2026-06-18
**父 RFC：** [RFC-20260604-mount-tree-legacy-api](./index.md)
**来源：** 初始背景调查 / 2026-06-18 文档层审查 / 2026-06-18 反馈机制重分类

本文只跟踪 design review 后确认的 mount 草案缺陷、证明缺口、边界冲突或需要回到草案修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围和阶段性交付项通常不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。受控实现反馈不新建通用 feedback 文件；计划写在 [迁移实施计划](./implementation.md#probe--vertical-slice-gates)，执行结果进入 transaction devlog。审查中明确选择为 limitation 的问题可在本文记录决策，但 canonical limitation text 仍必须落回 RFC / implementation / register。

分级沿用 Anemone review 口径：

- **Apollyon**：当前必须修复的错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态。
- **Keter**：会阻塞后续实现方向或导致核心抽象不可复审，必须修正或明确改边界。
- **Euclid**：值得修正，但通常不阻塞第一版实现。
- **Safe**：记录即可，除非顺手修正。

## Apollyon

- 暂无。

## Keter

- 暂无 active Keter。2026-06-18 审查项已折回 canonical 文本或受控反馈 gate，见 Neutralized。

## Euclid

- 暂无 active Euclid。模块边界预检已折回 [迁移实施计划](./implementation.md) 各阶段。

## Safe

- 暂无 active Safe。普通 `MS_REMOUNT` 的 accepted limitation 已归入 RFC 非目标、风险和阶段 3 / 7 计划。

## Neutralized

### BASELINE-20260608：grill-me 设计收敛后的草案基线

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 折叠 legacy API 范围、`MountTree` 正名、`PathRef` 位置模型、flag/data 分层、stack 可见性和暂缓边界。
- [不变量需求](./invariants.md) 折叠 bind/remount/unmount 生命周期和 transaction lock 边界。
- [迁移实施计划](./implementation.md) 折叠阶段边界和实现前置条件。

**结论：** 当前没有需要单独阻塞实现顺序的开放 tracking issue。

### BASELINE-20260618：多 agent 漂移检视后的同步修复

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 同步 mount API 范围、相邻 RFC 依赖和 accepted limitation 文本。
- [不变量需求](./invariants.md) 同步 cleanup、readonly 和 namei 相关约束。
- [迁移实施计划](./implementation.md) 同步阶段 gate、验证边界和实现顺序。
- [背景调查](./backgrounds/ltp-linux-reference-20260604.md) 同步 LTP / Linux reference 证据。

**结论：** fstype alias bridge、`/proc/mounts` live content、unmount cleanup 分层、per-mount readonly 写入口审计等漂移点已折叠回草案主文，当前不再保留 active design-formula tracking issue。

### KETER-003：阶段 3/4 执行依赖环

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 阶段 3 降级为 ordinary per-mount readonly remount 和 attr plumbing，不再宣称 `MS_REMOUNT | MS_BIND` 成功语义闭合。
- [迁移实施计划](./implementation.md) 阶段 4 在 bind view 语义存在后打开 `MS_REMOUNT | MS_BIND`，并验证 ro/rw sibling bind。

**结论：** 阶段顺序现在可以按 stage gate 关闭；bind-remount readonly 不再依赖尚未实现的 plain bind。

### KETER-004：attach 类 transaction 锁内 revalidation

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 legacy transaction 描述要求 `NewMount`、`BindMount`、`RecursiveBind` 在 transaction 内重验 target / source。
- [不变量需求](./invariants.md) 的线性化点要求 attach 类操作失败时不得发布新 view。
- [迁移实施计划](./implementation.md) 阶段 2 提供通用 revalidation helper，阶段 4 验证 bind/rbind TOCTOU 与 rollback。

**结论：** target/source revalidation 已成为正式不变量；`RecursiveBind` 的 source subtree snapshot / retry 细节作为阶段 4 反馈假设验证，但不能削弱全有或全无可见性。

### KETER-005：`MNT_DETACH` 后 deferred superblock cleanup owner

**状态：** Reclassified as Controlled Feedback Gate

**修复落点：**

- [RFC index](./index.md) 的接受边界声明 feedback 不能削弱目标或不变量。
- [不变量需求](./invariants.md) 明确 Gate P1 关闭前只能声明 topology detach，不能声明 lazy-detach 后 final `kill_sb` / observer cleanup 完整兼容。
- [迁移实施计划](./implementation.md#gate-p1---lazy-detach-final-cleanup-owner) 记录 P1 的 hypothesis、protected invariant、minimum write set、validation floor、failure signals、write-back 和 exit。

**结论：** 该问题不再作为前馈文档 blocker；它是阶段 6 的受控反馈 gate。若 P1 失败，阶段 6 只能登记 final cleanup limitation / open issue。

### KETER-006：detached / moved `PathRef` 的 namei、cwd/root 语义

**状态：** Reclassified as Controlled Feedback Gate

**修复落点：**

- [不变量需求](./invariants.md) 明确 detached / moved `PathRef` 不得 fallback 到当前全局 root 或 stale parent。
- [迁移实施计划](./implementation.md#gate-p2---detached-and-moved-pathref-namei-boundary) 记录 P2 的 hypothesis、protected invariant、minimum write set、validation floor、failure signals、write-back 和 exit。

**结论：** exact parent crossing 策略交给阶段 6 用 KUnit / targeted smoke 验证；不可削弱边界已经写入不变量，因此不再作为 active Keter。

### EUCLID-002：实现阶段模块边界预检

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 阶段 0-7 均补充模块边界预检，说明 split-only checkpoint、owner surface 和 write set 扩展边界。

**结论：** 模块边界预检已成为阶段计划的一部分，不再需要 active tracker。

### SAFE-001：普通 `MS_REMOUNT` 第一版只承诺 per-mount `RDONLY` 子集

**状态：** Rehomed as Accepted Limitation

**修复落点：**

- [RFC index](./index.md) 非目标和风险保留 ordinary remount 不做 sb-wide reconfigure 的限制。
- [迁移实施计划](./implementation.md) 阶段 3 要求相关 code comment 和 stable reject；阶段 7 负责公开 RFC / register closeout 时同步 limitation。

**结论：** 这是 accepted limitation，不是 active design issue；若在实现或 closeout 时仍存在，再同步到 `current-limitations`。

### SAFE-002：`SuperBlockInner.mounts` 的双真相源疑虑不保留为 active issue

**状态：** Neutralized

**结论：** 该字段按当前设计只应作为 `Weak<Mount>` 反向缓存，用于从 superblock 快速找到关联 mount view；它不得作为 mount topology、final-view 判断或 unmount 状态机的权威来源。权威 topology 和 cleanup 决策仍归 `MountTree`。

### EUCLID-001：阶段 1/7 的文档闭合职责重复

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 阶段 1 前置条件要求公开 RFC 进入实现状态，并建立 transaction devlog。
- [迁移实施计划](./implementation.md) 阶段 7 只负责 closeout、validation 和 limitation 记录更新。

**原问题：** 初始阶段计划把 RFC promotion / transaction devlog 建立与最终 closeout 混在同一条执行线上，容易让实现启动前缺少 canonical public plan，或在阶段 7 重复补齐早该关闭的文档 gate。

### KETER-001：mount identity 与 placement state 边界不清

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 把 `Arc<Mount>` identity 与 placement state 分层。
- [不变量需求](./invariants.md) 明确单一 placement lock 加 `placement_generation` retry 作为第一版 topology 发布机制。
- [迁移实施计划](./implementation.md) 要求 `MS_MOVE` 在同一 transaction 内重验 source/target、防环、修改旧 stack 和新 stack，并禁止 lookup 观察半移动状态。

**原问题：** 初始草案没有把 mount 对象身份和当前树位置分层，容易让 move / detach / lookup 共享 stale parent 或并列真相源。

### KETER-002：per-mount readonly attrs 缺少单一真相源

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 将 `MountAttrFlags` 固定为 `Mount` 上的 atomic bitset / interior-mutable attrs 单一真相源。
- [不变量需求](./invariants.md) 要求 remount 发布 attrs 前重验目标 view 仍 attached 且仍是当前 `MountTree` 目标 view。
- [迁移实施计划](./implementation.md) 要求旧 fd 对同一 live mount 的后续写必须观察 remount 后 attrs，detached 旧 view 不得被更新后返回成功。

**原问题：** 初始草案没有明确 readonly attrs 是 mount view 属性还是 file status / backend 属性，可能导致 old fd、detached view 和 remount 成功语义分裂。

### EUCLID-003：阶段 4/5 的 LTP smoke 可能误归因 `/proc/mounts` cleanup 失败

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 阶段 4/5 的条件验证说明。

**结论：** 阶段 4/5 的 LTP smoke 只能用于判断 bind / readonly / move 主语义，不得把 `/proc/mounts` cleanup 失败直接归因于这些主语义。
