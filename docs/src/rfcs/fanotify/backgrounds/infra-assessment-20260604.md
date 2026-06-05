# fanotify 基础设施评估

日期：2026-06-04

审查后注记：

- 本文是实施计划初稿前的基础设施评估，保留历史判断和素材索引；规范性阶段 gate 以后续 [tracking issues](../tracking-issues.md)、[不变量需求](../invariants.md) 和 [迁移实施计划](../implementation.md) 为准。
- 文中的 stage/phase 编排、`PathRef::location_eq()` 可用性、`FAN_CLOSE` bridge、目录项事件和 queue overflow 优先级已经被 tracking issues 收紧：Stage 0 不再是独立用户可见 gate，`FAN_CLOSE_*` 正式来源必须是 opened file description release，path/self/child 匹配必须使用 mount+dentry identity，dirent/name 类正向事件移入 FID/name follow-up，queue cap 前移到首个真实 VFS enqueue 前。
- 2026-06-05 后 ioctl-loop 和 fileops-seek-char-ioctl 已改变当前 file-op 事实：`FileOps` 现在包含 `read_at`、`write_at`、`seek` 和 `ioctl`，`sys_ioctl()` 已分发到 opened file 的 `FileOps::ioctl`。因此本文中关于 `FIONREAD` 可在 `sys_ioctl` 探测 fanotify private state 的历史选项已经废弃；canonical RFC 要求 fanotify `FIONREAD` 若实现，必须在 group fd `FileOps::ioctl` 内完成。

## 结论

当前内核基础设施已经足够启动一个有实际 LTP 得分价值的 fanotify stage-1：

1. 可以实现 `fanotify_init()` / `fanotify_mark()`。
2. 可以实现 fanotify group fd 的 `read()`、`poll()`、`close` 销毁。
3. 可以覆盖基础 path-fd 模式通知：`FAN_OPEN`、`FAN_ACCESS`、`FAN_MODIFY`、`FAN_CLOSE`；目录项/name 类正向 ABI 以现行 implementation 的 FID/name follow-up 边界为准。
4. 可以支持 inode / mount / filesystem 三类 mark 的基本匹配。

不建议首批追求完整 Linux 强一致语义。FID/name records、pidfd、permission event、`/proc/<pid>/fdinfo` 完整输出、FS error、evictable marks、精确 event merge/order 等都可以暂缓。只要首批把 unsupported feature 明确返回为 LTP 能识别的 `EINVAL`/`EPERM`/`ENODEV` 类结果，高阶用例会走 TCONF 或失败面会明显收窄。

## 已具备的基础

### 匿名 fd 与文件对象

现有匿名 inode 支持可以直接承载 fanotify group fd：

1. `fs::anonymous::{anony_new_inode, anony_open_with}` 已经能创建内核内部匿名文件。
2. `FileOps` 在本文写作时已有 `read`、`write`、`poll` vtable，fanotify group fd 可以用自己的 private state；2026-06-05 后还必须补齐 `read_at`、`write_at`、`seek`、`ioctl` 等 mandatory vtable 入口，具体 fail-closed 规则以现行 implementation 为准。
3. `Task::open_fd()` 支持 `FdFlags::CLOSE_ON_EXEC` 和共享 file status flags，足够实现 `FAN_CLOEXEC`、`FAN_NONBLOCK`、`event_f_flags` 的 stage-1 边界。
4. `FileStatusFlags::NONBLOCK` 已经存在，但 fanotify read 需要自己按 group fd flags 映射 `EAGAIN`。

需要补的只是 fanotify 自己的 `FileOps` 和 syscall handler，不需要先重做 fd 模型。

### VFS 身份与 mark 目标

现有 VFS 对象足以表达 fanotify 的三类目标：

1. `PathRef = Mount + Dentry`，适合 fanotify_mark 的 `dfd + pathname` 解析结果。
2. `InodeRef` 有稳定 inode identity，适合 inode mark。
3. `Mount` 已经是显式对象，适合 mount mark。
4. `SuperBlock` 挂在 `Mount` 上，适合 filesystem mark。
5. `Dentry` 父子关系足够提供 child/self 匹配输入；具体 identity 必须由现行计划要求的 mount+dentry key 或 `FanPathKey` 封装，不能直接依赖 `PathRef::location_eq()` 的旧 TODO 行为。

因此不需要先引入 Linux 形态的 `struct path` 或完整 fsnotify backend。需要的是一个 Anemone 自己的 mark registry，按 `InodeRef` / `Mount` / `SuperBlock` 挂 group mark。

### VFS 事件注入点

大部分基础事件都有集中入口：

1. `openat` 的 `finish_open()` 可以发 `FAN_OPEN`。
2. `File::read*()` 或 `FileDesc::read*()` 可以发 `FAN_ACCESS`。
3. `File::write*()` 已经在成功写入后调用 `after_modified(..., ModifType::Modify, ...)`，可以接 `FAN_MODIFY`。
4. `ftruncate` / `truncate` / `fchmod` / `fchown` / `utimensat` 已有集中 syscall 或 inode metadata 更新点，可后续接 `FAN_ATTRIB`。
5. `vfs_touch_at`、`vfs_mkdir_at`、`vfs_link`、`vfs_symlink_at`、`vfs_unlink_at`、`vfs_rmdir_at`、`vfs_rename_at` 都集中在 VFS 层，适合接 `FAN_CREATE`、`FAN_DELETE`、`FAN_MOVE`、`FAN_DELETE_SELF`、`FAN_MOVE_SELF`。

主要缺口是 `FAN_CLOSE`：当前 `close_fd()` 只是释放 fd 表引用，没有显式 file release hook。审查后不再把 fd-close 或 private-state `Drop` bridge 作为完整语义；正式 `FAN_CLOSE_*` 来源必须是 opened file description release，短期 bridge 只能标注为 temporary LTP bridge。

### poll / wait 基础

sched latch 与 typed poll registration 已经足够：

1. `PollRequest::{snapshot, register}` 和 `PollRegisterResult::{Ready, Armed, Unsupported}` 已经表达了 source readiness 与挂起注册。
2. pipe 已经有 `rx_poll_triggers` / `tx_poll_triggers` 的可参考模式。
3. fanotify group fd 只需要在 event queue 非空时返回 `READABLE`，在空队列 register 时保存 `LatchTrigger`，enqueue event 后 detach 并 trigger。

这部分基础设施足够，不需要等 epoll 或完整 Event 重构。

### credentials / capability

`CredentialSet` 和 `Capability::SYS_ADMIN` 已存在，可以做首批权限边界：

1. root / `CAP_SYS_ADMIN` listener 允许 path-fd 模式、mount mark、filesystem mark。
2. 非特权 listener 的 Linux FID-only 语义很复杂，建议首批不支持，返回 `EPERM` 或按 LTP feature probe 返回 `EINVAL`。

## 需要小补的基础设施

### ABI 常量与结构

目前 `anemone-abi` 只有 `SYS_FANOTIFY_INIT` 和 `SYS_FANOTIFY_MARK` syscall number，没有 fanotify UAPI 常量和结构体。需要新增：

1. `fanotify_event_metadata`
2. `fanotify_response`
3. 基础 `FAN_*` mask / mark / init flags
4. `FAN_EVENT_METADATA_LEN`、`FAN_NOFD`、metadata version 等固定值

强烈建议只把 Linux ABI 放在 syscall/ABI 边界，内核内部用语义类型：`FanMask`、`FanMarkFlags`、`FanGroupMode`、`FanEventKind`。

### group fd 的 FIONREAD

本文写作时 `sys_ioctl(FIONREAD)` 只识别 pipe。fanotify LTP 中可能会用到 fanotify fd 的 readable byte count。当前 canonical RFC 已采用后续落地的 `FileOps::ioctl` 边界：

1. 历史选项：短期在 `sys_ioctl` 内探测 fanotify private state。该选项已被现行 RFC 禁止。
2. 现行方向：通过已经落地的 `FileOps::ioctl` 在 fanotify group fd file ops 内处理 `FIONREAD`，未知命令返回 `UnsupportedIoctl` / `ENOTTY`。

这个不是首批事件主路径的硬前置，但补起来成本低。

### event fd 创建

path-fd 模式事件读出时，metadata 里的 `fd` 需要是一个新打开的对象 fd。现有 `PathRef::open()` 和 `Task::open_fd()` 能做到，但需要一个 fanotify 专用 helper：

1. 按 group 的 `event_f_flags` 解析 access/status/fd flags。
2. 从事件里的 `PathRef` 打开文件。
3. 插入当前读 fanotify fd 的 task fd table。
4. 避免这个内部 open 再生成 fanotify 递归事件。

FID 模式暂缓后，这个 helper 是 path-fd 模式能否通过基础用例的关键。

### mark registry 与锁

需要新增 `fs::fanotify` 或 `fs::notify::fanotify`：

1. 全局或 VFS-local registry，保存 group 与 mark。
2. group 内部 event queue、pending permission queue、poll trigger queue。
3. mark 需要记录 target identity、mask、ignored_mask、mark flags。
4. event enqueue 不能持有 VFS mutating lock 去打开 fd 或 copy user buffer，只应生成内核 event record 并唤醒 group。

stage-1 可以接受一个简单全局 `RwLock`/`SpinLock` registry。性能和复杂 event merge 后续再收敛。

## 建议暂缓的强一致点

这些不应该阻塞第一版：

1. FID/name records：`FAN_REPORT_FID`、`FAN_REPORT_DIR_FID`、`FAN_REPORT_NAME`、`FAN_REPORT_TARGET_FID`，需要 `name_to_handle_at` / exportfs 语义，当前没有基础。
2. pidfd：`FAN_REPORT_PIDFD` 依赖 pidfd file object，当前 `clone3(CLONE_PIDFD)` 也明确缺这个能力。
3. permission events：`FAN_OPEN_PERM`、`FAN_ACCESS_PERM`、`FAN_OPEN_EXEC_PERM` 需要打开路径阻塞、用户态回复、close 放行、signal/kill 交互。实现成本明显高于通知类事件。
4. `/proc/<pid>/fdinfo/<fanotifyfd>` 完整 mark 输出：对 fanotify09 等回归用例有用，但不是通知主路径。
5. queue overflow 精确语义：fixed queue cap 必须前移到首个真实 VFS enqueue 前；Linux 精确 `FAN_Q_OVERFLOW` 事件和合并语义可以后移。
6. event merge/order 精确一致性：LTP 基础用例会观察事件集合，完整 Linux merge 规则先不做。
7. unprivileged FID-only listener：Linux 语义强绑定 FID 模式，建议等 FID 后再做。
8. `FAN_FS_ERROR`：需要文件系统错误上报源，当前 ext4 层没有这个通知模型。
9. `FAN_MARK_EVICTABLE`：需要 inode cache eviction 语义，不适合首批。
10. 完整 `FAN_OPEN_EXEC` / `FAN_OPEN_EXEC_PERM`：需要 exec loader 打开路径上的专门事件点，可作为第二阶段。

## 对 LTP fanotify 的粗分层

### 首批应优先争取

1. `fanotify08`：`FAN_CLOEXEC`，基础设施已足够，成本低。
2. `fanotify04`：`FAN_MARK_ONLYDIR`、`FAN_MARK_DONT_FOLLOW`、`FAN_MARK_FLUSH`，主要是 mark 参数校验和路径解析。
3. `fanotify01` 的非 FID 基础路径：file open/access/modify/close、inode/mount/filesystem mark、ignore mask。
4. `fanotify02`：目录 child 基础事件，依赖 `FAN_EVENT_ON_CHILD`。
5. `fanotify12` 的普通 `FAN_OPEN` 部分；`FAN_OPEN_EXEC` 可暂缓或返回不支持让相关分支跳过。

### 第二阶段争取

1. `fanotify05`：queue overflow，需要 queue limit 和 `FAN_Q_OVERFLOW`。
2. `fanotify06` / `fanotify10`：inode + mount mark 与 ignore mask 合并，语义细但不依赖 FID。
3. `fanotify11`：`FAN_REPORT_TID`，如果愿意支持 thread id 报告，基础 task/tid 已有；否则让 feature probe TCONF。
4. `fanotify14` 的负例校验：大量是 ABI validation，适合跟 flag parser 一起补；stock `fanotify14` 可能先要求 FID setup，现行计划要求使用自建/裁剪 probe 或整体归入 FID 暂缓。
5. `fanotify17` 的 group/mark limit：可以用固定 limit 先实现，不做 user namespace 细节。

### 明确暂缓或让 TCONF

1. `fanotify03` / `fanotify07`：permission events。
2. `fanotify09`：包含 fdinfo ignore mask、FID/name、复杂 child/mount 回归。
3. `fanotify13` / `fanotify15` / `fanotify16`：FID/name/dirent records。
4. `fanotify18` / `fanotify19`：unprivileged FID listener。
5. `fanotify20` / `fanotify21`：pidfd。
6. `fanotify22`：FS error。
7. `fanotify23`：evictable marks。

## 历史实现边界建议

以下 phase 是本评估初稿的粗分层，已被现行 [迁移实施计划](../implementation.md) 的 Stage 0-5 gate 替代。保留它们只用于理解基础设施来源，不作为执行顺序。

### Phase 0: syscall 与 feature gating（历史）

目标是让 LTP probe 行为可控。

1. 新增 fanotify ABI 常量和 syscall handler。
2. `fanotify_init(FAN_CLASS_NOTIF, O_RDONLY/O_RDWR...)` 的公开成功在现行计划中推迟到 Stage 0 + Stage 1 合并 gate。
3. `fanotify_init(FAN_CLASS_CONTENT/PRE_CONTENT, ...)` 可以先成功创建 group，但 permission masks 在 `fanotify_mark()` 返回 `EINVAL`，这样 permission 用例会 TCONF，而不是在 `SAFE_FANOTIFY_INIT()` 处 TBROK。
4. `FAN_REPORT_FID/NAME/TARGET_FID/PIDFD/TID` 默认 `EINVAL`，除非当前阶段明确支持。
5. 非法 flag / event_f_flags 按参考手册做边界校验。

### Phase 1: group fd + inode/mount/filesystem mark（历史）

1. group fd private state：queue、marks、poll triggers、file status flags、event_f_flags。
2. mark add/remove/flush：先支持 basic mask、ignored_mask、ONLYDIR、DONT_FOLLOW、EVENT_ON_CHILD、ONDIR。
3. 支持 inode / mount / filesystem target identity。
4. 现行计划把 Stage 1 synthetic event 固定为 `FAN_NOFD`；path-fd metadata 和 event object fd 创建进入 Stage 3，并要求专用 read-user 提交协议。
5. `poll()` / `ppoll()` 可观察 queue readiness。
6. group teardown 发生在 shared group state / private state 最后 drop；不能把任意 fd number close 当作 teardown。`FAN_CLOSE_*` 另由 opened file description release 产生。

### Phase 2: VFS hook 覆盖（历史）

1. open/read/write/close 基础事件。
2. child/self 基础匹配；dirent/name 类正向事件进入 FID/name follow-up。
3. ignore mask 匹配。
4. mount/filesystem mark 匹配。
5. fixed queue cap 已前移；精确 `FAN_Q_OVERFLOW` 事件作为后续独立 gate。

### Phase 3: 扩展项（历史）

按得分和失败日志再决定是否做：

1. `FAN_REPORT_TID`
2. `/proc/<pid>/fdinfo/<fanotifyfd>` 最小输出
3. group/mark limit
4. `FAN_OPEN_EXEC`
5. permission events
6. FID/name records

## 风险判断

当前最大的风险不是基础设施不够，而是第一版范围过大。只做通知类 path-fd 模式时，现有 VFS/fd/wait/cred 基础足够；一旦把 FID、permission event、pidfd 和 fdinfo 强一致性一起纳入首批，就会变成跨 VFS、procfs、task、exec、pidfd、fs export 的大工程。

因此建议立即开工，但把首批验收定义为：

1. `fanotify_init/mark` 可 probe。
2. 基础通知队列可读、可 poll、可 nonblock。
3. inode/mount/filesystem mark 能匹配 open/read/write/close。
4. unsupported 高阶特性稳定返回 Linux 兼容错误，让 LTP 正确 TCONF 或形成明确剩余失败。
