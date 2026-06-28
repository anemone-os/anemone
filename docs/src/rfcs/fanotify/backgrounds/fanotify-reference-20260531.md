# Linux fanotify ABI 与客观语义参考手册

日期：2026-05-31

这是一份面向后续内核开发的 fanotify 参考手册，只记录对外 ABI、数据格式、返回值、权限边界和客观语义，不讨论 Anemone 内部实现。

## 1. 资料来源

主要依据以下材料整理：

1. Linux 6.6.32 UAPI `fanotify.h`。
2. Linux 6.6.32 `fanotify_user.c` 实现。
3. Linux 6.6.32 fanotify 内部头文件。
4. Linux 6.6.32 filesystem monitoring 文档。
5. Linux 6.6.32 procfs 文档。
6. Linux 6.6.32 fanotify 示例程序。
7. LTP 20240524 fanotify syscall 用例。

## 2. 术语

1. fanotify group：`fanotify_init()` 返回的匿名 inode fd，代表一个监听组。
2. mark：附着到 inode、mount 或 filesystem 的监视项。
3. notification event：普通通知事件，不需要用户态应答。
4. permission event：需要用户态写回响应的事件。
5. path mode：事件通过 `fd` 识别对象。
6. FID mode：事件通过 file handle / fsid 识别对象。
7. unprivileged listener：非 `CAP_SYS_ADMIN` 的监听组。

## 3. 系统调用与 fd 行为

fanotify 这套 ABI 主要由以下接口组成：

1. `fanotify_init(flags, event_f_flags)`
2. `fanotify_mark(fanotify_fd, flags, mask, dfd, pathname)`
3. `read(fanotify_fd, ...)`
4. `write(fanotify_fd, ...)`
5. `poll()/select()/epoll`
6. `close(fanotify_fd)`
7. `ioctl(FIONREAD)`，当前内核实现了该入口

### 3.1 `fanotify_init()`

作用：

1. 创建监听组。
2. 设定监听组类型、事件格式、资源上限和事件 fd 的默认打开标志。

`event_f_flags` 是给事件里返回的对象 fd 用的，不是 fanotify 组 fd 自己的 flags。

允许的 `event_f_flags` 只有：

1. `O_ACCMODE`
2. `O_APPEND`
3. `O_NONBLOCK`
4. `__O_SYNC`
5. `O_DSYNC`
6. `O_CLOEXEC`
7. `O_LARGEFILE`
8. `O_NOATIME`

`O_ACCMODE` 只能是 `O_RDONLY`、`O_WRONLY` 或 `O_RDWR`。

### 3.2 `fanotify_mark()`

作用：

1. 给 group 添加、删除或清空 mark。
2. 目标可以是 inode、mount 或 filesystem。

`pathname == NULL` 时，`dfd` 作为目标；否则按 `dfd + pathname` 解析。

### 3.3 `read()`

1. 一次 `read()` 可以返回多个完整事件。
2. 每个事件都以 `struct fanotify_event_metadata` 开头。
3. 缓冲区必须足够容纳完整事件，否则当前内核返回 `EINVAL`。
4. 事件从队列中取出后就不再保留。
5. 普通事件读出后立即销毁；permission event 读出后会进入待回复状态。

### 3.4 `write()`

1. `write()` 只用于 permission event 回复。
2. 回复格式是 `struct fanotify_response`，可附加额外 info record。
3. 回复 fd 必须匹配某个尚未完成的 permission event。

### 3.5 `close()`

1. 关闭 fanotify fd 会销毁 group。
2. 尚未回复的 permission event 会被视为允许通过。

## 4. `fanotify_init()` 约束

### 4.1 公开 flags

1. 类别：`FAN_CLASS_NOTIF`、`FAN_CLASS_CONTENT`、`FAN_CLASS_PRE_CONTENT`
2. 资源：`FAN_UNLIMITED_QUEUE`、`FAN_UNLIMITED_MARKS`
3. 审计：`FAN_ENABLE_AUDIT`
4. 事件格式：`FAN_REPORT_PIDFD`、`FAN_REPORT_TID`、`FAN_REPORT_FID`、`FAN_REPORT_DIR_FID`、`FAN_REPORT_NAME`、`FAN_REPORT_TARGET_FID`
5. fd 控制：`FAN_CLOEXEC`、`FAN_NONBLOCK`

约定宏：

1. `FAN_REPORT_DFID_NAME == FAN_REPORT_DIR_FID | FAN_REPORT_NAME`
2. `FAN_REPORT_DFID_NAME_TARGET == FAN_REPORT_DFID_NAME | FAN_REPORT_FID | FAN_REPORT_TARGET_FID`
3. `FAN_MARK_IGNORE_SURV == FAN_MARK_IGNORE | FAN_MARK_IGNORED_SURV_MODIFY`

### 4.2 互斥与依赖

1. `FAN_REPORT_PIDFD` 与 `FAN_REPORT_TID` 互斥。
2. 含有 FID 相关格式的初始化只能配 `FAN_CLASS_NOTIF`。
3. `FAN_REPORT_NAME` 依赖 `FAN_REPORT_DIR_FID`。
4. `FAN_REPORT_TARGET_FID` 依赖 `FAN_REPORT_NAME` 和 `FAN_REPORT_FID`。
5. `FAN_ENABLE_AUDIT` 只有在内核启用审计时才合法。

### 4.3 权限边界

1. 非 `CAP_SYS_ADMIN` 用户只能创建受限组。
2. 受限组只能使用 `FAN_CLASS_NOTIF`。
3. 受限组必须带 FID 报告模式，不能用纯 fd 事件。
4. 受限组不能请求 mount/filesystem mark。
5. 受限组不能请求 permission event。
6. 受限组不能请求 `FAN_REPORT_TID`、`FAN_REPORT_PIDFD`、`FAN_UNLIMITED_QUEUE`、`FAN_UNLIMITED_MARKS`。

### 4.4 典型返回值

1. `EINVAL`：非法 flag 组合、未知 bits、`event_f_flags` 非法、FID/CLASS 组合不对。
2. `EPERM`：权限不足。
3. `EMFILE`：group 数量上限。
4. `ENOSPC`：mark 数量上限。

## 5. `fanotify_mark()` 约束

### 5.1 mark 类型与命令

类型：

1. `FAN_MARK_INODE`
2. `FAN_MARK_MOUNT`
3. `FAN_MARK_FILESYSTEM`

命令：

1. `FAN_MARK_ADD`
2. `FAN_MARK_REMOVE`
3. `FAN_MARK_FLUSH`

修饰位：

1. `FAN_MARK_DONT_FOLLOW`
2. `FAN_MARK_ONLYDIR`
3. `FAN_MARK_IGNORED_MASK`
4. `FAN_MARK_IGNORED_SURV_MODIFY`
5. `FAN_MARK_EVICTABLE`
6. `FAN_MARK_IGNORE`

### 5.2 mask 分类

路径事件：

1. `FAN_ACCESS`
2. `FAN_MODIFY`
3. `FAN_CLOSE`
4. `FAN_OPEN`
5. `FAN_OPEN_EXEC`

permission 事件：

1. `FAN_OPEN_PERM`
2. `FAN_ACCESS_PERM`
3. `FAN_OPEN_EXEC_PERM`

目录项 / inode 事件：

1. `FAN_MOVED_FROM`
2. `FAN_MOVED_TO`
3. `FAN_MOVE`
4. `FAN_CREATE`
5. `FAN_DELETE`
6. `FAN_DELETE_SELF`
7. `FAN_MOVE_SELF`
8. `FAN_ATTRIB`
9. `FAN_RENAME`

其他：

1. `FAN_Q_OVERFLOW`
2. `FAN_FS_ERROR`
3. `FAN_EVENT_ON_CHILD`
4. `FAN_ONDIR`

### 5.3 关键限制

1. `mask` 只使用低 32 位。
2. `FAN_MARK_ADD` 和 `FAN_MARK_REMOVE` 不能带空 `mask`。
3. `FAN_MARK_FLUSH` 只能和 mark 类型位一起用。
4. 非法 bit 组合返回 `EINVAL`。
5. mount/filesystem mark 需要 `CAP_SYS_ADMIN`。
6. permission event 只能出现在 content/pre-content 组里。
7. `FAN_FS_ERROR` 只能配 filesystem mark。
8. `FAN_MARK_EVICTABLE` 只对 inode mark 有意义。
9. 依赖 file handle 的事件不能挂在 mount mark 上。
10. `FAN_RENAME` 需要 `FAN_REPORT_NAME`。
11. `FAN_MARK_ONLYDIR` 遇到非目录返回 `ENOTDIR`。
12. 非目录 inode 上的 `FAN_RENAME`、`FAN_ONDIR`、`FAN_EVENT_ON_CHILD`、以及某些 `FAN_MARK_IGNORE_SURV` 组合会返回 `ENOTDIR`。
13. `FAN_MARK_IGNORE` 与 `FAN_MARK_IGNORED_MASK` 不能同时出现。
14. `FAN_MARK_IGNORED_MASK` 下，`FAN_ONDIR` 和 `FAN_EVENT_ON_CHILD` 对 ignored mask 不起作用。
15. `FAN_MARK_IGNORE` 作用于 mount、filesystem、目录 inode 时，必须带 `FAN_MARK_IGNORED_SURV_MODIFY`。
16. mount / filesystem mark 上使用 `FAN_MARK_IGNORE` 不带 `FAN_MARK_IGNORED_SURV_MODIFY`，返回 `EINVAL`。
17. 目录 inode 上使用 `FAN_MARK_IGNORE` 不带 `FAN_MARK_IGNORED_SURV_MODIFY`，返回 `EISDIR`。
18. `FAN_MARK_DONT_FOLLOW` 让符号链接标记自身，而不是目标。

### 5.4 语义边界

1. `FAN_MARK_MOUNT` 标记的是路径所在挂载，而不是单个 inode。
2. `FAN_MARK_FILESYSTEM` 标记的是整个 filesystem。
3. `FAN_EVENT_ON_CHILD` 只描述目录的立即子项，不递归到子目录的子项。
4. `FAN_ONDIR` 表示事件发生在目录对象上；目录项修改场景里它与 `FAN_EVENT_ON_CHILD` 是两类不同语义。
5. `FAN_MARK_IGNORE` 和 `FAN_MARK_IGNORED_MASK` 都是忽略掩码相关语义，但前者更明确地让 `FAN_ONDIR` / `FAN_EVENT_ON_CHILD` 影响忽略匹配。

## 6. 事件数据格式

### 6.1 `struct fanotify_event_metadata`

当前 metadata 版本是 3。

字段语义：

1. `event_len`：当前事件的完整长度，包含 metadata、info records 和对齐填充。
2. `vers`：版本号，当前为 3。
3. `metadata_len`：基础 metadata 长度，当前就是 `sizeof(struct fanotify_event_metadata)`。
4. `mask`：事件 mask。
5. `fd`：对象 fd，或 `FAN_NOFD`。
6. `pid`：生成事件的 task ID；是否是 thread id 取决于 `FAN_REPORT_TID`。

### 6.2 迭代规则

1. 用 `FAN_EVENT_OK(meta, len)` 检查完整性。
2. 用 `FAN_EVENT_NEXT(meta, len)` 走到下一条事件。
3. `event_len` 是下一条记录的偏移，不要按固定结构大小硬切。
4. `info record` 也必须按 `hdr.len` 跳过未知类型。

### 6.3 `fd` 与特殊值

1. `FAN_NOFD == -1`
2. `FAN_NOPIDFD == FAN_NOFD`
3. `FAN_EPIDFD == -2`

`FAN_Q_OVERFLOW` 和 `FAN_FS_ERROR` 都不携带普通对象 fd。

### 6.4 事件合并

1. 同一对象、同一来源的连续事件可以合并为一个队列项。
2. permission event 不合并。
3. overflow event 不合并。

## 7. 信息记录

所有 info record 都以 `struct fanotify_event_info_header` 开头：

1. `info_type`
2. `pad`
3. `len`

用户态必须按 `len` 前进，不能依赖 record 顺序。

### 7.1 文件标识记录

1. `FAN_EVENT_INFO_TYPE_FID`
2. `FAN_EVENT_INFO_TYPE_DFID`
3. `FAN_EVENT_INFO_TYPE_DFID_NAME`

语义：

1. `fsid + file_handle` 唯一标识对象。
2. `DFID_NAME` 在 handle 后面附带一个 NUL 结尾的名字。
3. handle 是 opaque `struct file_handle` 内容，可交给 `open_by_handle_at()`。

### 7.2 目录 / rename 记录

1. `FAN_EVENT_INFO_TYPE_OLD_DFID_NAME`
2. `FAN_EVENT_INFO_TYPE_NEW_DFID_NAME`

语义：

1. 只给 `FAN_RENAME` 这类事件使用。
2. 可能同时出现 old 和 new 两套 dir+name 记录。
3. 解析时不要把“先后顺序”当作 ABI。

### 7.3 `FAN_EVENT_INFO_TYPE_PIDFD`

1. 记录里带一个 pidfd。
2. `FAN_REPORT_PIDFD` 时使用。
3. 如果事件来源任务在 pidfd 创建前已经退出，记录里会是 `FAN_NOPIDFD`。
4. 其他 pidfd 创建失败记为 `FAN_EPIDFD`。

### 7.4 `FAN_EVENT_INFO_TYPE_ERROR`

1. 只用于 `FAN_FS_ERROR`。
2. `error` 是 errno 值。
3. `error_count` 统计为保留首个错误而压制掉的后续错误数。
4. 这类事件的目标是“报告文件系统出错”，不是证明一次 I/O 成功完成。
5. 当前文档树里提到的典型 emitter 是 ext4。

### 7.5 `FAN_REPORT_NAME` 的一个细节

如果 group 开了 `FAN_REPORT_NAME`，但某个目录事件没有天然的名字可报，内核会报告 `"."`。

## 8. permission event 回复

`struct fanotify_response`：

1. `fd`
2. `response`

合法回复值：

1. `FAN_ALLOW`
2. `FAN_DENY`
3. `FAN_AUDIT`
4. `FAN_INFO`

回复约束：

1. `response` 的低位必须是 `ALLOW` 或 `DENY`。
2. `FAN_AUDIT` 需要 group 启用 audit。
3. `FAN_INFO` 时可以附带 `FAN_RESPONSE_INFO_AUDIT_RULE`。
4. `fd` 必须对应一个仍在等待回复的 permission event。
5. 写错 fd 通常返回 `ENOENT`。
6. 关闭 fanotify fd 时，未回复的 permission event 会被放行。

## 9. `/proc/<pid>/fdinfo` 可见性

`/proc/<pid>/fdinfo/<fanotifyfd>` 里能看到：

1. `fanotify flags`
2. `event-flags`
3. 每个 mark 的 `mflags`
4. 每个 mark 的 `mask`
5. 每个 mark 的 `ignored_mask`
6. 如果内核支持 exportfs，还会显示 `fhandle-bytes`、`fhandle-type`、`f_handle`

这部分是排障和验证接口，不是额外 syscall。

## 10. 本仓现有 fanotify 测试覆盖

当前 LTP 用例覆盖的重点如下：

1. 基本 inode / mount / filesystem mark。
2. child 事件与 `FAN_EVENT_ON_CHILD`。
3. permission event 的 allow / deny。
4. `FAN_MARK_ONLYDIR`、`FAN_MARK_DONT_FOLLOW`、`FAN_MARK_FLUSH`。
5. queue overflow。
6. ignore mask 的合并和生效。
7. 关闭 listener 时 permission event 的行为。
8. `FAN_CLOEXEC`。
9. `FAN_REPORT_TID`。
10. 非法 flag / mask 组合的 errno。
11. directory entry modification events 与 `FAN_REPORT_DFID_NAME_TARGET`。
12. group / mark 限额。
13. unprivileged listener 的限制。
14. `FAN_REPORT_PIDFD`。
15. `FAN_FS_ERROR`。
16. `FAN_MARK_EVICTABLE`。

## 11. 记录原则

1. 这里只记录事实，不写实现路线。
2. `FAN_ALL_*` 这类旧宏只当兼容接口看，不当作可继续扩展的 ABI 边界。
3. 解析事件时只信 `info_type`、`len`、`event_len`，不信位置假设。
4. 如果后续要补实现设计，先回到这份文档核对 ABI，再写方案。
