# fanotify 不变量需求

**状态：** Draft
**最后更新：** 2026-06-10
**父 RFC：** [RFC-20260604-fanotify](./index.md)

## 闭合条件

- `fanotify_init()` 创建的 group fd 有单一 group state，`read()`、`poll()`、mark 操作和 close teardown 都引用同一状态。
- mark registry 是 fanotify matching 的单一真相源；event source hook 不复制 mark 状态。
- `fs::fanotify` 是 group、registry、queue、mark record、file ops private state 和 matching 的 owner；模块外只能通过 typed facade 交互，不能 downcast fanotify private state 或访问内部 lock/storage。
- fanotify 内部状态使用 Anemone 语义类型；Linux UAPI struct 只在 syscall 参数解析和 group fd read/write copy 边界出现。
- event source hook 只生成 fanotify event queue item，不在 hook 内打开事件 fd、copy user buffer 或等待 permission response。
- event queue readiness 与 poll trigger 注册/触发在线性化点内闭合，不能 lost wake。
- group fd 是 stream-like fanotify object：普通 read/readv 可以消费 queue，poll 只观察 readiness，可选 ioctl 只处理 group fd 命令；`seek`、`read_at`、`write_at` 和 `read_dir` 必须 fail closed，不能消费 queue、创建 metadata fd 或改变 fanotify 状态。
- group fd nonblock read 必须观察当前 opened file description `FileStatusFlags`；`F_SETFL(O_NONBLOCK)` 后续变更必须影响 empty read。fanotify private nonblock 副本只能作为临时 bridge，不能作为首个公开 gate 的闭合证据。
- event queue 必须在首个真实 fanotify enqueue 前有固定 cap 和 overflow sentinel；无界队列、只统计不丢弃或 silent fail-open 都不能作为 path-fd event 阶段的实现状态。
- path-fd event 的对象 fd 只在 `read(fanotify_fd)` 时为当前读取 task 创建。
- path-fd read 的 event 消费、event fd 安装、metadata copyout 和失败回滚/丢弃策略必须是单个可复审提交协议；copyout 失败不得留下用户不可见的新 fd。
- 暂缓特性不能伪成功；必须稳定返回 Linux-compatible unsupported/invalid/permission errno。
- group semantic teardown 必须发生在最后一个用户可见 descriptor / opened-description ref 关闭时，移除 group marks、唤醒 blocking read / poll waiters，并释放 queue / pending state；该 teardown 必须由 task/fd 层显式 close / exit / fd-table replacement 生命周期路径触发，不能由 `Drop`、memory last-drop 或 deferred task dispose 触发。memory last-drop 只负责最终内存释放或断言遗漏的显式清理。
- `FAN_CLOSE_*` 事件来源必须是被监控对象的 opened file description release；任意 fd number close 只能作为临时 bridge，不能算完整 release 语义。

## 非目标

- 不证明完整 Linux fsnotify mark connector 语义。
- 不证明完整 event merge/order。
- 不证明 FID/name、pidfd、permission event、FS error 或 evictable mark 语义。
- 不证明非特权 FID-only listener 的 Linux 兼容性。

## 状态所有权

`fs::fanotify` 拥有 group、mark、event queue、poll trigger queue、registry storage、file ops private state、matching 语义和 fanotify syscall API。VFS、task/fd、procfs 和 syscall dispatch 层只能通过窄 typed API 与它交互。fanotify syscall API 放在 `anemone-kernel/src/fs/fanotify/api/`，不放在 `anemone-kernel/src/fs/api/fanotify/`；`fs/api` 不新增 fanotify 子目录，也不承载 fanotify parser、errno matrix 或 group/mark 逻辑。

固定模块骨架如下：

```text
anemone-kernel/src/fs/fanotify/
├── mod.rs
├── types.rs
├── api/
│   ├── mod.rs
│   ├── init.rs
│   └── mark.rs
├── group.rs
├── file.rs
├── event.rs
├── queue.rs
├── registry.rs
├── mark.rs
└── hooks.rs
```

职责边界：

- `mod.rs`：唯一模块外 facade；re-export syscall-facing `api::{sys_fanotify_init, sys_fanotify_mark}`、user-visible event hook API、task/fd-facing release / no-notify helper 和必要 opaque handle。
- `types.rs`：跨子模块共享的 Anemone 语义类型，例如 mask、flag、mode、event kind、target key 和 path key；不放 Linux UAPI struct。
- `api/init.rs`：`fanotify_init()` 参数解析、init flag matrix、event fd flags validation、group creation facade 调用和 fd install 入口。
- `api/mark.rs`：`fanotify_mark()` 参数解析、mark command/mask validation、path resolution 入口和 registry facade 调用。
- `group.rs`：group identity、state、lifecycle、closing/dead、mark handle list 和 teardown。
- `file.rs`：fanotify group fd `FileOps`、private-state downcast、read/poll/ioctl/fail-closed vtable 和 read-user glue。
- `event.rs`：event kind、queue item、metadata build input 和 path-fd event description。
- `queue.rs`：bounded queue、overflow sentinel、poll trigger queue 和 wake detachment。
- `registry.rs`：global registry、mark record storage、target maps、matching snapshot 和 cleanup by handle。
- `mark.rs`：mark record、`MarkHandle`、mask/ignored-mask operations 和 target-dead state。
- `hooks.rs`：user-visible event input functions；只收集事实并调用 registry/matching facade。

global registry、group lock、queue、mark record、private state cast 和 fanotify-specific ioctl/read/write interpretation 都必须留在 owner 模块内。模块外不得直接访问这些 storage，也不得使用 `AnyOpaque` 判断 fanotify concrete type。

`fs/fanotify/api/` 拥有 fanotify Linux ABI 参数解析：

- `fanotify_init(flags, event_f_flags)` 解析 group class、report mode、fd flags 和 event fd flags。
- `fanotify_mark(fanotify_fd, flags, mask, dfd, pathname)` 解析 mark command、target type、mark flags、event mask 和 path resolution 规则。
- 用户态 `read()` 只在 fanotify group fd file ops 中解释 fanotify metadata。`write()` 只有在 permission event gate 引入 pending response queue 后才解释 permission response ABI；首批通知类实现必须对 permission response write 返回稳定 unsupported / invalid errno，不能静默成功。
- fanotify group fd 的 `ioctl()` 只在 group fd file ops 中解释可选命令，例如 `FIONREAD`；未知 ioctl 返回 `UnsupportedIoctl` / `ENOTTY`。`sys_ioctl()` 不得 downcast fanotify private state 或新增 fanotify-specific 分支。
- fanotify group fd 的 `seek`、`read_at`、`write_at` 和 `read_dir` 不属于 fanotify ABI；这些入口必须返回稳定非支持错误，`pread` / `pwrite` 不得绕过 read-user 提交协议或产生 permission response 语义。
- syscall 层不得通过 `AnyOpaque` downcast 判断某个 fd 是否 fanotify；它只能调用 fd/file 层提供的 typed operation 或 fanotify syscall facade。

fanotify event hook 拥有事件发生点的事实输入：事件种类、当前 task、target path、parent path、name、directory/self/child 属性。它不拥有 group state，也不直接决定 userspace event layout。

syscall/API helper 与 task/fd lifecycle 拥有 fanotify 基础事件注入边界。`FAN_OPEN` 由 open API 在对象权限、flag validation 和 `O_TRUNC` 副作用处理完成后、fd 发布前提交。`FAN_ACCESS` 必须覆盖用户可见 fd 的顺序 read 和 positioned read 成功路径；`FAN_MODIFY` 必须覆盖用户可见 fd 的顺序 write、positioned write、append、fd/path truncate、fallocate grow 或等价内容修改成功路径。`FAN_CLOSE_*` 来源是 task/fd opened-description final-release。事件源可以调用 `fs::fanotify` 的 typed hook，但不得把 fanotify policy 下沉到 `fs::File`、具体 backend `FileOps::{read,read_at,write,write_at}`、char/block/proc backend 或 loop/ext4 backing I/O。`fs::File` 是内核内部 opened object handle；它不得 import fanotify、持有 notification suppression policy、提供 `*_opened()` 这类用户 fd 语义 wrapper，或为了 fanotify 进入 registry/no-notify 等 sleepable lock。

task/fd 层拥有 fd table、fd reservation、file description lifetime 和 user-visible fd event suppression marker。fanotify 可以通过受控 helper 在 read 边界安装 event object fd，但不得保存用户态 fd number 作为长期身份，也不得维护与 fd table 并行的 descriptor/open-state 真相源。path-fd read 使用的 fd reservation 必须独占一个未发布 fd slot；commit 前其他 fd 分配和 close 路径都不能观察或复用该 slot，commit 只发布已准备好的 file、不得再分配/阻塞/失败，rollback 在 commit 前幂等释放 slot 和 file。reserved slot open-state 必须由 task/fd 层与普通 fd table 状态统一维护。opened-description final-release 是 task/fd 层的显式语义释放事件，必须在可睡眠、interrupts-enabled 的 close / exit / fd-table replacement 路径运行；fd table、task 或 group 的 `Drop` 只能释放内存或断言显式释放遗漏，不得运行 fanotify registry mutation、waiter wakeup 或其它系统资源释放。`FAN_CLOSE_NOWRITE` / `FAN_CLOSE_WRITE` 需要从 opened file description release 取得事件事实，而不是从 group fd close 或 fd table slot close 推断。每个被监控 opened file description 必须保存 fanotify close snapshot：对象 `PathRef` / `FanPathKey` 和打开时 access mode 是否包含写能力；dup/fork 共享同一 opened file description 时只在最后 release 产生一次 close event。后续成功内容修改状态只服务 `FAN_MODIFY` 和 legacy ignore-mask clearing，不参与 close mask 分类。event fd 的 suppression marker 只由 syscall/API helper 或 task/fd lifecycle 在提交 fanotify event 前检查，不能作为参数继续下沉到 `fs::File`。

## 身份与能力模型

fanotify group identity 是匿名 fd private state，而不是 fd number。dup/fork 后多个 fd description 可以引用同一个 group；mark 和 queue 必须挂在 shared group state 上。group lifecycle 分成两层：最后一个用户可见 descriptor / opened-description ref 关闭时，在 task/fd 显式生命周期路径进入 semantic closing/dead，阻止新 enqueue、移除 marks、唤醒 blocking read / poll waiters；内存释放可以等 in-flight syscall 持有的 transient refs 归零。blocking `read()` 自己持有 transient ref 时，不能阻止 semantic close wakeup。析构路径不得补做 semantic close；若 shared group state 或 fd table drop 时仍需要 semantic teardown，说明上游显式释放路径遗漏，应以断言暴露。

mark target 使用内部身份：

- inode mark：`InodeRef` identity。
- mount mark：`Arc<Mount>` identity。
- filesystem mark：`Arc<SuperBlock>` identity。

这些 identity 必须封装为 `FanTargetKey::{Inode, Mount, SuperBlock}` 或等价 key 类型，集中定义相等和 hash 规则。首批可以使用封装后的 pointer identity 或对象内 stable id，但 callsite 不得直接比较裸指针。registry 至少区分 `marks_by_target` 与 group-owned cleanup handles，保证 close / flush / remove 只删除本 group 的 mark。`FanMarkRecord` 只能由 registry arena / slot map 或等价 owner 持有；target map 和 group cleanup list 只能保存 `MarkHandle`，不得复制 event mask、ignored mask、target refs、`target_dead` 或 generation。registry 不得强拥有 group；mark entry 只能保存 `GroupId + generation + Weak/non-owning group handle` 或等价非拥有引用。matching 在 registry lock 下解析 group handle，解析失败、generation 不匹配或 group 已 dead 时跳过；registry entry 不能延长 group 生命周期。

首批 mark target lifecycle 采用强引用 + pre-unmount flush / mark-dead。mark 和 queued event 可以 pin `InodeRef` / `Arc<Mount>` / `Arc<SuperBlock>`，但完整 umount/filesystem kill 兼容必须等 VFS 在 umount 或 filesystem kill 前调用 fanotify 清理相关 marks、标记 target dead 后才能宣称。late enqueue 观察到 dead target 或 dead group 必须 fail closed。

event target 使用 `PathRef` 或明确 parent/name snapshot。path-fd mode 需要能在 read 时打开事件对象；如果对象已删除或不能打开，按 fanotify path-fd 语义返回 `FAN_NOFD`，不得 panic。path/self/child 匹配必须使用 `FanPathKey { mount, dentry }` 或等价 mount+dentry identity；不得依赖只比较 dentry 的 helper 作为 fanotify identity。

poll producer capability 使用 `LatchTrigger`，只保存到 group queue 中。fanotify source 不能直接拿到 wait-core `WakeToken` 或 task scheduling API。

## 线性化点

`fanotify_init()` 的线性化点是 group state 完整初始化并安装到当前 task fd table。失败路径不得留下 registry mark 或半初始化 fd。Stage 0 单独提交时只能作为内部 ABI/probe checkpoint；公开成功 gate 必须同时包含最小 group fd read/poll/close wakeup 和 bounded queue 行为。

`fanotify_mark(ADD)` 的线性化点是 mark 插入 registry。插入前必须完成 flag/mask/target validation；插入失败不得改变已有 mark，除非命令语义明确是更新已有 mark。

`fanotify_mark(REMOVE)` 的线性化点是 mark mask 或 mark entry 从 registry 移除。remove 未命中必须返回稳定 errno，不能静默成功。

`fanotify_mark(FLUSH)` 的线性化点是 group 对应 target class 的 marks 被移除。flush 不应影响其他 group。

event matching snapshot 是 event 的线性化点，并且必须与 ADD / REMOVE / FLUSH 由同一 registry lock 序列化。remove / flush 返回后，后续 matching snapshot 不得再命中被删除 mark；已经在 snapshot 中命中的 event 可以继续尝试 append。进入 group 前按固定 group id 顺序处理目标 groups；enqueue 在 group lock 下最后检查 group dead、target dead、queue cap 和 overflow sentinel，再 append 或丢弃。若 queue 从 empty 变为 non-empty，必须在同一 group lock 临界区内 detach poll triggers，释放 lock 后触发。

`read(fanotify_fd)` 的线性化点由 fanotify 专用 read-user 提交协议定义。group lock 下选择完整 event 并出队，普通通知 event 的消费点就是出队点，首批不保留重读。path-fd event fd 必须通过可回滚的 fd 预留/安装 helper 提交：fd reservation 独占未发布 slot，metadata copyout 全部成功后 commit，commit 必须不可失败；copyout 失败时 rollback 并释放已创建 file。对象打开失败输出 `fd = FAN_NOFD` 并继续 copyout。一个 metadata record 不得半提交；若本次 read 已经提交前面的完整 records，后续 record 失败只能 rollback 未提交 fd，并返回已提交字节数。首批采用这一简化返回策略，Linux `EFAULT` 细节差异必须在验证中记录。permission event 首批不支持。Stage 1 synthetic event 使用 `fd = FAN_NOFD`，不触发 path-fd event fd 创建。

generic read-user hook 只是 opened-description 提供 direct userspace copyout 的能力，不天然代表被观察文件内容读取。ordinary `FAN_ACCESS` 只能由声明为 read-user access source 的 opened description 或普通 backend read 成功边界提交；fanotify group fd 的 control read-user 必须关闭该能力，避免 group fd 被标记后通过 `read(fanotify_fd)` 递归生成队列事件。

`poll(fanotify_fd)` register 的线性化点是 group lock 下检查 queue readiness，并在仍 empty 时保存 trigger。若 queue 已非空，必须直接返回 ready，不得保存 trigger 后再要求等待。

group semantic teardown 的线性化点是最后一个用户可见 descriptor / opened-description ref 关闭时，由 task/fd 显式 close / exit / fd-table replacement 路径按 `registry -> group` 锁序标记 closing/dead 并从 registry 移除所有 marks。之后 event source hook 不得再向该 group enqueue 新事件。teardown 必须按 group-owned handles 删除本 group marks、detach blocking read / poll waiters、把 queue 内容搬出锁外释放。shared group state 的实际内存释放可以延后到 in-flight syscall transient refs 归零；不能把 memory last-drop 当作 close wakeup 的唯一触发点，也不能让 `Drop` / deferred task dispose 触发需要普通 `Mutex`、waiter trigger 或 registry mutation 的语义释放。`FAN_CLOSE_*` 事件另有独立线性化点：被监控 opened file description 的 release。

## 锁序与生命周期规则

fanotify registry lock 不得包住用户态 copy、event object fd open、VFS path open、large queue drops 或可能阻塞的操作。

固定锁序：

1. syscall/API helper 或 task/fd lifecycle 收集事件输入。
2. fanotify registry lock 下做 target dead check、mark matching snapshot、mask / ignored mask 计算。
3. 按固定 group id 顺序进入每个 group lock，重查 group dead、target dead、queue cap 和 overflow sentinel，append event 或丢弃，并 detach poll triggers。
4. 释放 lock。
5. trigger waiters。

fanotify registry lock 不得包住 group queue 的大规模 drop；group lock 不得包住用户 copy、event object fd open、VFS path open、wait 或 trigger 执行。

event queue item 持有的引用必须足够支撑后续 read 边界，不得保存裸指针或短生命周期 borrowed path。若保存 `PathRef` 会延长对象生命周期，必须受 queue cap 约束，并在 group teardown 时把 queue 内容搬出锁外释放。

group teardown 必须先按 `registry -> group` 顺序阻止新 enqueue，再清理 marks 和 queue，最后唤醒 blocking read / poll waiters。teardown 不能先持有 group lock 再回头获取 registry lock；queue 内容和 trigger 列表可以在锁内搬出，但引用释放和 trigger 执行必须在锁外完成。late hook 观察到 dead group 或 dead target 必须丢弃，不得 panic。

`Drop` 路径不得获取 fanotify registry lock、group lock 或 task/fd semantic-release 相关锁来补做系统资源释放。fd table / group / queue 的析构只能释放已经脱离可见系统状态的内存；若析构时仍发现 published fd、reserved fd、live mark 或未完成 semantic teardown，应使用断言暴露生命周期 bug，而不是在析构中继续清理。

no-notify 当前采用 opened-description 持久 suppression 模型：fanotify event-fd helper 只走不提交用户可见 open event 的内部 `PathRef::open()` / `fs::File` 路径，因此不保留构造期 `NoNotifyGuard`。返回给用户的 event object fd 必须在 opened file description 上携带 kernel-only 通用 notification suppression 标记，使该 fd 后续经 syscall/API helper 或 opened-description final-release 时跳过 fanotify enqueue。file 标记只属于 fanotify 生成的 event fd，不得传播到普通用户 open，也不得绕过普通 VFS 权限、生命周期或用户可见 I/O 语义；task/fd 核心不得依赖 fanotify 具体 private type。

## unsupported feature 边界

首批暂缓项必须 fail closed：

- `FAN_REPORT_FID` / `FAN_REPORT_DIR_FID` / `FAN_REPORT_NAME` / `FAN_REPORT_TARGET_FID`：默认 `EINVAL`。
- `FAN_REPORT_PIDFD`：默认 `EINVAL`。
- `FAN_REPORT_TID`：支持前默认 `EINVAL`，除非同阶段补齐 pid/tid 字段切换语义。
- `FAN_UNLIMITED_QUEUE` / `FAN_UNLIMITED_MARKS`：首批默认 `EINVAL`，不得忽略成功；真实支持必须有对应资源上限语义和 LTP 分类更新。
- `FAN_ENABLE_AUDIT`：无 audit backend 时默认 `EINVAL`。
- permission event mask：默认在 `fanotify_mark()` 返回 `EINVAL`，让 helper-based permission feature probe 能 TCONF；stock direct `SAFE_FANOTIFY_MARK()` 失败按暂缓项分类。
- permission response write：permission event gate 前默认返回稳定 unsupported / invalid errno，不得创建无 pending event 的假回复语义。
- `FAN_OPEN_EXEC` / `FAN_OPEN_EXEC_PERM`：exec hook gate 前默认 `EINVAL`，不得被 basic event mask parser 接受。
- dirent/name/self/metadata event mask：`FAN_CREATE`、`FAN_DELETE`、`FAN_MOVED_FROM`、`FAN_MOVED_TO`、`FAN_RENAME`、`FAN_DELETE_SELF`、`FAN_MOVE_SELF`、`FAN_ATTRIB` 在对应 path-fd observable ABI 或 FID/name gate 前默认 `EINVAL`，不得进入 registry。
- 非特权 pure path-fd listener：默认按权限规则返回 `EPERM`，不得创建不完整 listener。
- 非特权 FID-only listener：在 FID mode 未支持前按 init/report feature gate 返回 `EINVAL`，不得创建不完整 group。
- `FAN_FS_ERROR`：默认 `EINVAL` 或 filesystem unsupported。
- `FAN_MARK_EVICTABLE`：默认 `EINVAL`。
- `FAN_MARK_IGNORE` 新语义：首批默认 `EINVAL`；legacy `FAN_MARK_IGNORED_MASK` / `FAN_MARK_IGNORED_SURV_MODIFY` 可以作为 basic ignore mask 支持。

如果某个暂缓项改为支持，必须在同一阶段补齐对应 observable ABI，不得只接受 flag。

## 禁止退化项

- 在各 syscall/API 事件源中直接遍历 fanotify group 内部结构。
- 在 syscall、event source hook、task/fd 或 procfs 层 downcast fanotify group fd private state，绕过 `fs::fanotify` facade。
- 在 fanotify 内部长期保存 Linux ABI struct 作为核心状态。
- 在 event enqueue hook 中创建 event object fd 或 copy user buffer。
- poll register 在 queue 已 ready 时仍保存 trigger 并返回 `Armed`。
- source 保存 trigger 后依赖 cleanup 才能保证正确性；late trigger 必须由 wait identity fail closed。
- fanotify 内部 open 事件 fd 时递归生成 fanotify event，或 fanotify 生成的 event fd 后续 I/O / close 再次生成 fanotify event。
- event-fd no-notify 标记泄漏到非 fanotify event fd，或绕过 VFS 权限检查。
- 在 task/fd 核心中加入 fanotify-specific concrete type 判断，而不是通用 notification suppression / release hook 能力。
- 在 `fs::File` 或 backend `FileOps` 中保留 fanotify import、notification suppression 参数、`*_opened()` wrapper 或其它用户 fd 可见性 policy。
- unsupported high-level feature 返回成功但事件格式或行为不可观测。
- group fd close 后 registry 仍保留 live mark。
- target map、group cleanup list 或 group state 中复制 mark mask / ignored mask / target refs，形成 registry 之外的第二份 mark 真相源。
- path-fd metadata copyout 失败后留下用户无法观察也无法关闭的新 event fd。
- 用 fd table slot close、fanotify group fd close 或 private-state drop bridge 冒充 `FAN_CLOSE_*` opened file description release，并据此宣称 Stage 3 闭合。
- 在 FID/name reporting 继续暂缓时，正向输出 dirent/name 类假事件。

## 完成标准

第一阶段可以声明闭合，当且仅当：

- syscall probe、flag validation、group fd lifecycle、blocking read/nonblock/poll/close wakeup 和 basic mark add/remove/flush 均有验证证据。
- open/read/write/close 基础事件能经过 inode / mount / filesystem mark 匹配进入 queue。
- path-fd read 提交协议、queue cap、registry key/lifecycle 和 `FAN_CLOSE_*` opened file description release source 均有闭合证据；temporary fd-close bridge 只能作为降级实现记录，不能作为第一阶段闭合证据。
- fanotify module/API 可见性、mark record 单一 owner、task/fd fd reservation 单一真相源、通用 no-notify file 标记和 `fs::File` fanotify-agnostic 边界均有源码级或审查证据。
- unsupported feature 的 errno 策略有 LTP probe 证据或源码级验证。
- 剩余失败能被归类到明确暂缓项，而不是基础通知队列或 mark registry 缺陷。
