# LTP fanotify 测例调查报告

日期：2026-06-05

本文调查 LTP 20240524 中 fanotify 相关测例，并把它们映射到本 RFC 的 staged fanotify 草案。本文只记录测例事实、依赖、预计归类和对后续验收的影响，不替代主 RFC、实现计划或 tracking issues。

## 资料来源

- LTP 20240524 fanotify syscall tests。
- LTP 20240524 fanotify helper `fanotify.h`。
- LTP 20240524 fanotify LAPI definitions。
- Anemone runner fanotify 组：`anemone-apps/user-test/ltp/groups/fanotify.txt`
- 当前 fanotify RFC：[index](../index.md)、[implementation](../implementation.md)、[invariants](../invariants.md)

`anemone-apps/user-test/ltp/groups/fanotify.txt` 当前列出 `fanotify01` 到 `fanotify23`，即 stock LTP fanotify syscall 组全量用例；另有 helper 程序 `fanotify_child` 被 permission / exec-open / ignore 相关测例复制执行。

## 总体结论

LTP fanotify 组不是单纯的 `fanotify_init()` / `fanotify_mark()` smoke。23 个测例覆盖了基础 path-fd 通知、mark 生命周期、ignore mask、child/on-dir、queue overflow、priority class、permission response、FID/name records、pidfd records、unprivileged listener、resource limit、procfs fdinfo、FS error 和 evictable mark。

对当前 Anemone staged plan 的直接含义：

- 首批 path-fd 目标可以覆盖 `fanotify01` 的非 FID 分支、`fanotify02`、`fanotify04`、`fanotify08`、`fanotify11` 的无 `FAN_REPORT_TID` 分支，以及 `fanotify05` 的 limited-queue 分支。
- stock `fanotify05` 全测例还包含 `FAN_UNLIMITED_QUEUE` 分支；该分支没有 helper probe。如果 Stage 0 按当前计划拒绝 `FAN_UNLIMITED_QUEUE`，该分支会从 `SAFE_FANOTIFY_INIT()` 走 TBROK，而不是 TCONF。
- 为了让 permission 用例通过 helper probe 变成 TCONF，`FAN_CLASS_CONTENT` / `FAN_CLASS_PRE_CONTENT` 需要能创建 group，但 permission mask 在 `fanotify_mark()` 返回 `EINVAL`。这也会激活 `fanotify06` / `fanotify10` 里的 content/pre-content notification class 场景；如果 priority / ignore 语义不完整，它们会变成 TFAIL，而不是 TCONF。
- `fanotify09` 和 `fanotify10` stock 形态依赖 `/proc/<pid>/fdinfo/<fanotifyfd>` 中的 fanotify mark 输出；如果完整 fanotify fdinfo 仍是非目标，这两项不能作为第一阶段 stock 通过目标。
- `fanotify13` 到 `fanotify16` 是 FID/name/dirent 重点用例；`fanotify14` 虽然包含 invalid flag / errno 矩阵，但 setup 阶段先 `REQUIRE_FANOTIFY_INIT_FLAGS_SUPPORTED_ON_FS(FAN_REPORT_FID, ...)`，所以在 FID 暂缓阶段不能用 stock `fanotify14` 证明非 FID 负例。
- `fanotify17` 到 `fanotify23` 主要是资源上限、unprivileged FID listener、pidfd、FS error 和 evictable mark；它们均不适合作为首批 path-fd gate。

## LTP helper 语义

这些 helper 决定 unsupported feature 在 LTP 输出中是 TCONF、TBROK 还是 TFAIL：

- `SAFE_FANOTIFY_INIT(flags, event_f_flags)`：只有 `fanotify_init()` 返回 `ENOSYS` 时会 TCONF；其它失败会 TBROK。因此，stock 用例中直接 `SAFE_FANOTIFY_INIT(FAN_UNLIMITED_QUEUE)` 或 `SAFE_FANOTIFY_INIT(FAN_CLASS_PRE_CONTENT)` 时，如果返回 `EINVAL`，结果是 TBROK。
- `SAFE_FANOTIFY_MARK(...)`：任何 `fanotify_mark()` 失败都会 TBROK。只有用例在 setup 或 case 前先走 `fanotify_flags_supported_on_fs()` / `fanotify_mark_supported_on_fs()` 并主动 TCONF，unsupported errno 才能被归类为 TCONF。
- `fanotify_flags_supported_on_fs(init_flags, mark_flags, mask, path)`：`fanotify_init()` 的 `EINVAL` 被视为 init flag unsupported；`fanotify_mark()` 的 `EINVAL` 被视为 mark/mask unsupported；`ENODEV`、`EOPNOTSUPP`、`EXDEV` 被视为 filesystem unsupported；其它 errno 会 TBROK。
- `REQUIRE_FANOTIFY_INIT_FLAGS_SUPPORTED_ON_FS()` / `REQUIRE_FANOTIFY_EVENTS_SUPPORTED_ON_FS()`：把上述 unsupported 结果转成 TCONF 并终止测例。

因此，当前草案的 “暂缓项 fail closed” 需要逐个对照用例是否有 helper probe。没有 probe 的 stock 分支不能期望自动 TCONF。

## 测例逐项调查

| 测例 | 覆盖主题 | 关键依赖 | 对当前 plan 的含义 |
| --- | --- | --- | --- |
| `fanotify01` | 基础文件事件：`FAN_OPEN`、`FAN_ACCESS`、`FAN_MODIFY`、`FAN_CLOSE_*`；inode / mount / filesystem mark；legacy ignored mask 清除和 survive modify；FID 变体。 | path-fd metadata、返回 event fd 可读、pid、basic ignore mask、mount/filesystem mark、`FAN_REPORT_FID` 变体。 | 非 FID 分支是 Stage 2/3 主目标；FID 分支有 helper probe，`FAN_REPORT_FID` 返回 `EINVAL` 时可 TCONF。 |
| `fanotify02` | 目录子项事件：目录 inode mark + `FAN_EVENT_ON_CHILD` / `FAN_ONDIR`，随后 remove child mask，确认只剩目录自身事件。 | child matching、`FAN_MARK_REMOVE`、basic open/read/write/close event。 | Stage 2/3 主目标；能暴露 parent/child 匹配和 remove 线性化错误。 |
| `fanotify03` | permission events：`FAN_OPEN_PERM`、`FAN_ACCESS_PERM`、`FAN_OPEN_EXEC_PERM`，用户态写 `fanotify_response` allow/deny。 | `FAN_CLASS_CONTENT`、permission queue、blocking open/access、response write、exec permission。 | permission gate 暂缓。当前阶段应让 `require_fanotify_access_permissions_supported_on_fs()` 在 mark 阶段看到 `EINVAL` 并 TCONF。 |
| `fanotify04` | special mark flags：`FAN_MARK_ONLYDIR`、`FAN_MARK_DONT_FOLLOW`、`FAN_MARK_FLUSH`；group fd nonblock empty read 返回 `EAGAIN`。 | symlink target/self 区分、flush、returned fd stat、nonblock read。 | Stage 1/2/3 高价值 smoke；`ONLYDIR` 非目录失败不检查 errno，但必须失败。 |
| `fanotify05` | queue overflow：limited queue 产生 `FAN_Q_OVERFLOW`；unlimited queue 不应 overflow。 | `FAN_NONBLOCK`、mount mark、`/proc/sys/fs/fanotify/max_queued_events` 或默认 16384、overflow sentinel、event order。 | limited branch 是 Stage 1/3 资源上限目标；unlimited branch 无 helper probe，若 `FAN_UNLIMITED_QUEUE` 返回 `EINVAL` 会 TBROK，需要记录为 stock 暂缓或另行过滤。 |
| `fanotify06` | priority class 与 ignored mask 合并：pre-content / content / notif 三类 group，mount mark + ignored mask；overlayfs 变体。 | `FAN_CLASS_PRE_CONTENT`、`FAN_CLASS_CONTENT`、priority ordering、ignored mask、overlay mount。 | 不是首批 path-fd gate。若为了 permission probe 接受 content/pre-content group，本测例会运行并要求 priority / ignore 语义。 |
| `fanotify07` | permission events 在 instance destruction 时的清理：未回复 permission event、关闭 group、另一个 instance teardown 不应 hang/crash。 | permission pending queue、response write、group teardown、child kill。 | permission gate 暂缓；mark `FAN_ACCESS_PERM` 返回 `EINVAL` 可让 setup TCONF。 |
| `fanotify08` | `FAN_CLOEXEC` flag sanity：`fcntl(F_GETFD)` 检查 `FD_CLOEXEC`。 | `fanotify_init()` flag parser、fd close-on-exec bit。 | Stage 0/1 最小 stock 目标。 |
| `fanotify09` | parent/subdir/mount mark 的 child / on-dir / ignore 合并逻辑；部分 case 使用 `FAN_REPORT_DFID_NAME`；部分 case 使用 `FAN_MARK_IGNORE`。 | multiple groups、`FAN_EVENT_ON_CHILD`、`FAN_ONDIR`、legacy ignore、new `FAN_MARK_IGNORE`、optional name record、proc fdinfo ignore mark 检查。 | stock 不是首批目标；legacy 子集也需要 fdinfo 和较细 child/ignore 语义。 |
| `fanotify10` | 大型 ignore matrix：inode/mount/filesystem mark 交叉、bind mount、priority classes、legacy ignore 与 `FAN_MARK_IGNORE` 双 variant、exec-open、evictable ignore、fdinfo mark 计数。 | content/pre-content/notif classes、filesystem mark、bind mount、`FAN_OPEN_EXEC`、`FAN_MARK_EVICTABLE`、`/proc/<pid>/fdinfo`、`drop_caches`、`vfs_cache_pressure`。 | stock 不是首批目标。可作为后续 ignore/priority/fdinfo gate 的分解来源，不能整案放入 Stage 3 验收。 |
| `fanotify11` | `FAN_REPORT_TID`：无该 flag 时 event pid 是 tgid；有该 flag 时 event pid 是触发线程 tid。 | pthread、`FAN_ALL_EVENTS | FAN_EVENT_ON_CHILD`、pid/tid metadata。 | 无 `FAN_REPORT_TID` 分支可作为 Stage 3 pid=tgid smoke；`FAN_REPORT_TID` 返回 `EINVAL` 时第二分支 TCONF。 |
| `fanotify12` | `FAN_OPEN_EXEC` 与 `FAN_OPEN` 的区分，以及 ignored mask 对 exec-open 的影响。 | exec path event hook、`fanotify_child` helper、`FAN_OPEN_EXEC` probe。 | exec-open gate 暂缓。即使 `FAN_OPEN_EXEC` unsupported，纯 `FAN_OPEN` case 仍可能期待 exec 路径产生 `FAN_OPEN`，需谨慎归类。 |
| `fanotify13` | `FAN_REPORT_FID` 基础：event fd 应为 `FAN_NOFD`，FID 与 `statfs()` / `name_to_handle_at()` 一致；inode/mount/filesystem mark；overlay variants。 | file handle、fsid、`name_to_handle_at()`、overlayfs、FID event info record。 | FID gate 暂缓。 |
| `fanotify14` | FID/name 相关 invalid flag / invalid mask errno 矩阵，包含 `EINVAL`、`ENOTDIR`、`EISDIR` 等。 | `FAN_REPORT_FID` setup requirement、`FAN_REPORT_NAME` dependency、target fid、pipes、SELinux EACCES 容忍。 | stock 整案需要 FID，不能在 FID 暂缓阶段证明非 FID 负例；当前 tracking issue FANOTIFY-014 的结论仍成立。 |
| `fanotify15` | `FAN_REPORT_FID` 下 dirent / self event：`FAN_CREATE`、`FAN_DELETE`、`FAN_MOVE`、`FAN_DELETE_SELF`、`FAN_MODIFY`，以及 merge 行为。 | FID records、dirent/self masks、event merge、filesystem mark。 | FID/dirent gate 暂缓。 |
| `fanotify16` | FID/name/report-target/rename 综合：`FAN_REPORT_DFID_NAME`、`FAN_REPORT_DIR_FID`、`FAN_REPORT_DFID_FID`、`FAN_REPORT_DFID_NAME_TARGET`、`FAN_RENAME`。 | name records、old/new dfid name records、target fid、rename merge/order、mount + filesystem + inode marks。 | FID/name/rename follow-up；复杂度远高于首批 path-fd。 |
| `fanotify17` | group / mark resource limits；user namespace 内全局与 per-userns limit。 | `FAN_REPORT_FID` required for user listener、`/proc/sys/fs/fanotify/max_user_*`、`/proc/sys/user/max_fanotify_*`、`unshare(CLONE_NEWUSER)`、`RLIMIT_NOFILE`。 | resource-limit / unprivileged gate 暂缓；环境依赖重。 |
| `fanotify18` | unprivileged listener forbidden flags 和 mark 权限错误。 | drop 到 `nobody`、`FAN_REPORT_FID` required、`EPERM` for disallowed init/mark。 | unprivileged FID listener 暂缓；FID unsupported 时 setup TCONF。 |
| `fanotify19` | unprivileged listener 事件格式：事件应 `fd = FAN_NOFD`；child 事件 pid 期望 0；privileged reader 变体。 | unprivileged FID-only group、pid redaction、event fd suppression。 | unprivileged FID listener 暂缓。 |
| `fanotify20` | `FAN_REPORT_PIDFD` init flag validation：与 `FAN_REPORT_TID` 互斥；与 FID/name 组合可成功。 | pidfd report flag、init errno matrix。 | pidfd gate 暂缓；若 `FAN_REPORT_PIDFD` unsupported，setup TCONF。 |
| `fanotify21` | `FAN_REPORT_PIDFD` event info record：self event 返回有效 pidfd，已退出 child event 返回 `FAN_NOPIDFD`。 | pidfd info record、`pidfd_open()`、`/proc/self/fdinfo/<pidfd>`。 | pidfd + procfs gate 暂缓。 |
| `fanotify22` | `FAN_FS_ERROR`：通过 debugfs / corrupted filesystem 触发 FS error event，校验 error info 和 FID。 | `FAN_FS_ERROR`、FID records、`debugfs` 命令、mount remount abort、filesystem corruption。 | FS error gate 暂缓；对 Anemone 当前环境成本很高。 |
| `fanotify23` | evictable inode marks：升级/降级 errno、drop_caches 后 mark evict、`FAN_ATTRIB` ignore。 | `FAN_MARK_EVICTABLE`、`FAN_REPORT_FID`、`FAN_ATTRIB`、`/proc/sys/vm/drop_caches`、mount cycle。 | evictable + attr/self metadata gate 暂缓。 |

## 按阶段归类

### Stage 0 + Stage 1 可用 stock 信号

- `fanotify08`：验证 `FAN_CLOEXEC` 到 fd flags 的映射。
- `fanotify04` 的 nonblock no-event 读：能观察 empty nonblock read 返回 `EAGAIN`。
- `fanotify05` limited branch：能观察 bounded queue 和 `FAN_Q_OVERFLOW`。注意 stock binary 还会继续 unlimited branch。

Stage 1 的 blocking read / poll / close wakeup 没有被这些 stock 测例完整覆盖；仍需要自建 smoke 或后续内核内 probe。

### Stage 2 + Stage 3 首批 path-fd 候选

- `fanotify01` 非 FID 的 inode/mount/filesystem mark 分支。
- `fanotify02` 目录 child/on-dir 分支。
- `fanotify04` `ONLYDIR`、`DONT_FOLLOW`、`FLUSH`。
- `fanotify11` 无 `FAN_REPORT_TID` 分支。

这些候选仍要求：

- `FAN_CLOSE_*` 来自 opened file description release，而不是 fd table slot close。
- `FAN_MARK_IGNORED_MASK` / `FAN_MARK_IGNORED_SURV_MODIFY` 至少覆盖 `fanotify01` 的 legacy 行为。
- path-fd `read(fanotify_fd)` 能返回可用 event fd，并且 fanotify 内部 event fd 不自激生成递归事件。

### 复杂 ignore / priority / fdinfo gate

- `fanotify06`、`fanotify09`、`fanotify10` 应从第一阶段 stock 通过目标中排除。
- 它们适合作为后续分解 gate 的资料来源：priority class、ignore mask inheritance、child/on-dir 细节、bind mount identity、fdinfo mark 输出。
- 如果当前阶段继续把完整 `/proc/<pid>/fdinfo/<fanotifyfd>` 作为非目标，则 `fanotify09` / `fanotify10` 至少有部分 case 会 TFAIL。

### 明确暂缓 gate

- permission：`fanotify03`、`fanotify07`
- exec-open：`fanotify12`
- FID/name/dirent/rename：`fanotify13`、`fanotify14`、`fanotify15`、`fanotify16`
- resource limit / unprivileged listener：`fanotify17`、`fanotify18`、`fanotify19`
- pidfd：`fanotify20`、`fanotify21`
- FS error：`fanotify22`
- evictable mark / attr/self metadata：`fanotify23`

## 对当前 RFC 的校准点

1. `FAN_UNLIMITED_QUEUE` 首批返回 `EINVAL` 是合理的 fail-closed 语义，但 stock `fanotify05` 不会 TCONF。事务日志应把该分支记录为 stock 暂缓失败，或在 runner 层单独管理。

2. `FAN_CLASS_CONTENT` / `FAN_CLASS_PRE_CONTENT` 不能简单拒绝，否则 permission helper 的 `SAFE_FANOTIFY_INIT(FAN_CLASS_CONTENT, ...)` 会 TBROK。当前草案选择 “class 可建 group、permission mask 在 mark 阶段 `EINVAL`” 是符合 LTP probe 的；代价是 `fanotify06` / `fanotify10` 会开始测试 content/pre-content notification priority。

3. `fanotify14` 的 stock 用例不适合作为 Stage 0 invalid matrix 验收。需要自建或裁剪 probe 来覆盖非 FID negative cases；stock `fanotify14` 只能等 FID gate 后再进入。

4. `/proc/<pid>/fdinfo/<fanotifyfd>` 不是纯辅助输出。`fanotify09` / `fanotify10` 会读取其中的 `fanotify ... mflags ... mask ... ignored_mask` 行来判断 ignored mask 是否存活、evictable marks 是否被回收。因此 fdinfo 若暂缓，应在 LTP 分类中明确。

5. `FAN_OPEN_EXEC` 暂缓不仅影响 `FAN_OPEN_EXEC` mask。`fanotify12` 的纯 `FAN_OPEN` case 也通过 `execve()` 触发 open 路径，可能要求 exec loader open 产生普通 `FAN_OPEN`。如果 Anemone exec path 不走普通 VFS open hook，需要把该差异单独记录。

6. FID/name 支持一旦打开，会大量激活 LTP：`fanotify01` 的 FID 分支、`fanotify13` 到 `fanotify16`、`fanotify17` setup、`fanotify18` / `19` unprivileged listener、`fanotify20` 的 pidfd+FID 组合、`fanotify22` / `23`。因此 FID gate 必须包含 file handle、fsid、event info record layout 和 `name_to_handle_at()` 兼容性，不能只接受 init flag。

## 建议的 LTP 使用方式

首批实现验证不要直接宣称 stock `fanotify.txt` 全组通过。更稳妥的分层是：

1. 内部 smoke：自建 probes 覆盖 `fanotify_init()` errno matrix、blocking/nonblock read、poll、close wakeup、queue overflow sentinel。
2. 第一批 stock/case-level 观察：`fanotify08`、`fanotify04`、`fanotify02`、`fanotify01` 非 FID 分支、`fanotify11` 无 TID 分支；`fanotify05` limited branch 单独记录，unlimited branch按暂缓分类。
3. 第二批 ignore/priority/fdinfo：从 `fanotify06`、`fanotify09`、`fanotify10` 中拆子目标，不把 stock 全案作为一个 gate。
4. 后续 feature gates：permission、FID/name、pidfd、unprivileged、FS error、evictable mark 分别建 gate，避免一个 feature flag 提前成功后激活大批 TBROK/TFAIL。

## 后续文档挂钩

本报告支持当前 RFC 中的以下边界：

- Stage 0 不作为独立用户可见 LTP gate。
- Stage 0 + Stage 1 是第一个公开 gate。
- FID/name、pidfd、permission events、FS error、evictable marks、exec-open events 继续作为非首批目标。
- stock `fanotify14` 不能作为非 FID negative validation。
- queue cap / overflow sentinel 需要前移到首个真实 enqueue 之前。
