# mount LTP 与 Linux 参考调查

**状态：** Background
**日期：** 2026-06-04
**父 RFC：** [RFC-20260604-mount-tree-legacy-api](../index.md)
**参考来源：**

- LTP：本地 2024-05 参考快照
- Linux：6.6.32 参考快照
- Anemone 当前实现：`anemone-kernel/src/fs`、`anemone-abi/src/fs.rs`

## 调查结论

mount 相关工作不能只按 `mount(2)` 的 C syscall 用例估算。LTP 直接覆盖面包括：

- `runtest/syscalls` 中 22 个 mount-family 条目：
  - `mount01` 到 `mount07`
  - `umount01` 到 `umount03`
  - `umount2_01`、`umount2_02`
  - `fsopen01`、`fsopen02`
  - `fsmount01`、`fsmount02`
  - `open_tree01`、`open_tree02`
  - `move_mount01`、`move_mount02`
  - `mount_setattr01`
  - `pivot_root01`
- `runtest/fs_bind` 中 95 个非注释条目：
  - plain bind：25 个
  - move：22 个
  - recursive bind：40 个
  - cloneNS：7 个
  - regression：1 个
- `runtest/fs_readonly` 中 55 个 `test_robind.sh` 条目，统一依赖正常 mount、bind mount、`remount,ro,bind` 后再运行 growfiles/rwtest/ftest 等文件系统压力工具。
- `testcases/kernel/containers/mountns` 另有 `mountns01` 到 `mountns04`，测试 shared/private/slave/unbindable 在 `CLONE_NEWNS` 下的传播行为。

因此首要结论是：普通 `mount(2)` 参数支持只是入口，真正的测试压力来自 mount topology。尤其是 bind / rbind / move / propagation / namespace 的组合，不应该被实现为 `sys_mount()` 的 flag 特判。

## Anemone 当前基线

当前实现已经具备：

- `sys_mount()` 权限检查：要求 `CAP_SYS_ADMIN`。
- source 解析：`NULL` source 视为 pseudo；非空 source 视为路径并要求指向 block device。
- filesystem type 解析：通过 `get_filesystem()` 找到注册 filesystem；截至 2026-06-18，syscall 边界把 `tmpfs`、`ext2`、`ext3`、`vfat` 归一化为 `ramfs`，其中 `ext2` / `ext3` / `vfat` 是为了 LTP 的临时兼容桥，不应下沉为 mount tree 或 filesystem backend 框架语义。
- 新 mount attach：`mount_at(fs_name, source, flags, &target)` 创建 superblock root dentry 和 `Mount`，挂到 visible namespace。
- `sys_umount2()` 权限检查、目标必须是 mount root、调用 `unmount()`。
- `unmount()` 拒绝 root mount，拒绝有 child mount 的 mount，最后一个 superblock 使用者退出时尝试 evict/kill superblock。
- `Mount::ensure_writable()` 已用 `MountFlags::RDONLY` 作为写路径只读 enforcement 的入口。

当前缺口：

- `anemone-abi/src/fs.rs` 只导出 `MS_RDONLY`。
- `MountFlags` 只有 `RDONLY`。
- `sys_mount()` 除 `MS_RDONLY` 外把其他 flag 记录为 unsupported 并忽略。
- fstype alias bridge 仍缺少明确日志和退出条件，可能掩盖真实 filesystem support；mount tree RFC 需要把它限制在 syscall adapter。
- 没有 `MS_BIND`、`MS_REC`、`MS_MOVE`、`MS_REMOUNT`、`MS_SHARED`、`MS_PRIVATE`、`MS_SLAVE`、`MS_UNBINDABLE` 分流。
- 没有 mount propagation group、peer group、master/slave、unbindable 状态。
- visible namespace 是全局 singleton，不是 task-local mount namespace。
- 没有 detached mount object，因此无法自然表达 `open_tree(OPEN_TREE_CLONE)`、`fsmount()` 的 mount fd 或 `move_mount()` attach。
- `umount2()` 不解析 `MNT_DETACH`、`MNT_EXPIRE`、`UMOUNT_NOFOLLOW`，也没有 expire mark。
- `sys_mount()` 当前用普通 lookup 解析 target；Linux `umount` 对 `UMOUNT_NOFOLLOW`、mountpoint lookup、symlink follow 有更细边界。
- `mount data` 参数目前未使用；这对 ext4 basic mount 可能够用，但 new mount API 的 `fsconfig(source=...)` 会需要受控参数状态。

## LTP syscall 组拆解

### `mount01`

基础正例：对多个 filesystem 执行 `mount(tst_device->dev, MNTPOINT, tst_device->fs_type, 0, NULL)`。

要求：

- 测试设备基础设施可提供 block device。
- filesystem type 可识别。
- target mountpoint 是目录。
- 成功后能由测试框架清理 umount。

### `mount02`

基础 errno 负例：

- 错误 filesystem type：期望 `ENODEV`。
- 已 mounted folder：期望 `EBUSY`。
- `MS_REMOUNT | MS_RDONLY` 但 mountpoint 内有文件等场景：期望 `EBUSY`。
- invalid device / invalid filesystem type：期望 `EINVAL`。
- mounted folder + `MS_REMOUNT`：期望 `EINVAL`。
- fault device / fault fs type：期望 `EFAULT`。
- long path：期望 `ENAMETOOLONG`。
- nonexistent target：期望 `ENOENT`。
- target 是 file：期望 `ENOTDIR`。

要求：

- `mount(2)` parser 必须先做稳定 flag/type/source/target validation，不能把 unsupported remount 忽略后成功。
- `EBUSY` 与 `EINVAL` 区分会被 LTP 直接观察。

### `mount03`

flag 正例和行为验证：

- `MS_RDONLY`：写入返回 `EROFS`。
- `MS_NODEV`：访问 device special file 受限。
- `MS_NOEXEC`：执行 mount 上的文件返回 `EACCES`。
- `MS_REMOUNT`：从 readonly remount 回可写。
- `MS_NOSUID`：suid/sgid 不生效。
- `MS_NOATIME`、`MS_NODIRATIME`、`MS_STRICTATIME`：验证 atime 行为或 `statfs/statvfs` flags。

要求：

- 这些 flag 大多是 per-mount attribute，不能只保存在 superblock。
- 当前 `RDONLY` 已有 enforcement 入口；`NOEXEC`、`NODEV`、`NOSUID`、`NOSYMFOLLOW` 需要分别接入 exec、device open、credentials/setuid、path traversal。
- atime flags 如果文件系统没有 atime 更新机制，需要稳定分类为暂缓或以可观察 flag 策略处理，不能 silent pass。

### `mount04`

非特权用户调用 `mount()` 期望 `EPERM`。当前 `CAP_SYS_ADMIN` 检查方向正确，但后续 mount namespace / user namespace 不应放宽这个边界。

### `mount05`

plain `MS_BIND` 正例：把 `MNTPOINT1` bind 到 `MNTPOINT2`，验证两个路径下文件和目录都可访问。

要求：

- bind mount 不是创建新 filesystem superblock，而是复制同一个 mount/dentry view 到新 mountpoint。
- bound view 的 root 可以是原 mount 的子 dentry，不一定是 superblock root。
- unmount bind mount 不应 kill shared superblock 或影响 source mount。

### `mount06`

`MS_MOVE` 正例：先把临时路径 bind 自己并设为 private，再把已有 mountpoint 移动到另一个 mountpoint，验证旧路径不可见、新路径可访问。

要求：

- move 是 detach + attach 同一个 mount tree，不是新建 clone。
- 需要防止把 mount 移到自身子树造成拓扑环。
- 需要处理 propagation 约束；至少第一版要明确 unsupported matrix 的 errno。

### `mount07`

`MS_NOSYMFOLLOW` 行为：普通 mount 下 symlink 可跟随，remount 加 `MS_NOSYMFOLLOW` 后 symlink traversal 失败并返回 `ELOOP`。

要求：

- path resolution 必须能读取当前 mount 的 no-symlink-follow 属性。
- 这个 flag 不是 syscall lookup 的 `AT_SYMLINK_NOFOLLOW`，而是 mounted tree 上后续 lookup 的行为约束。

### `umount01` 到 `umount03`

覆盖基础 `umount()`：

- 正常 unmount 成功。
- busy mount 返回 `EBUSY`。
- fault path 返回 `EFAULT`。
- nonexistent path 返回 `ENOENT`。
- 非 mountpoint 返回 `EINVAL`。
- long path 返回 `ENAMETOOLONG`。
- 非特权返回 `EPERM`。

要求：

- Anemone 当前 `sys_umount2()` 对非 mount root 返回 `NotMounted`，需要确认 errno 映射是否为 Linux 期望的 `EINVAL`。
- busy 判断不能只看 child mount；还要处理 cwd/root/open file/inode refs 的可观察占用策略。

### `umount2_01`

`MNT_DETACH` 正例。测试会 lazy detach mountpoint，并验证 mountpoint 不再出现在正常可见挂载中。

要求：

- lazy detach 应把 mount tree 从 namespace 中摘掉，但允许已有引用之后自然释放。
- detach 后 path lookup 不应再到达该 mount。
- detached mount 不应再被普通 `/proc/mounts` 类视图列出。

### `umount2_02`

`MNT_EXPIRE` 和 `UMOUNT_NOFOLLOW`：

- `MNT_EXPIRE | MNT_FORCE` 返回 `EINVAL`。
- `MNT_EXPIRE | MNT_DETACH` 返回 `EINVAL`。
- 首次 `MNT_EXPIRE` 返回 `EAGAIN`。
- 访问后再次 `MNT_EXPIRE` 仍返回 `EAGAIN`。
- 第二次未访问的 `MNT_EXPIRE` 成功。
- `UMOUNT_NOFOLLOW` 作用在 symlink target 时返回 `EINVAL`。

要求：

- mount object 需要 expire mark。
- path lookup 必须能按 `UMOUNT_NOFOLLOW` 控制最后一跳 symlink。
- expire 行为需要定义引用计数和访问清除 mark 的策略；第一版如果暂缓，需要稳定返回 unsupported/invalid，不要误成功。

## `fs_bind` 组拆解

`fs_bind` 是 mount work 的主要压力源。公共库 `fs_bind_lib.sh` 做以下事情：

- 创建 `sandbox`，把它 bind 到自身，再通过 `mount --make-private` 等命令设置传播类型。
- 创建四个目录树 `disk1` 到 `disk4` 作为可比较的内容源。
- 每个用例用 `mount --bind`、`mount --rbind`、`mount --move`、`mount --make-{shared,private,slave,unbindable}` 构造拓扑。
- 用 `diff -r` 检查不同路径下的树内容是否传播一致。
- cleanup 通过 `/proc/mounts` 找出 sandbox 下挂载并逆序 umount。
- cloneNS 用例通过 `tst_ns_create mnt` / `tst_ns_exec ... mnt` 在 mount namespace 中执行操作。

覆盖矩阵：

| 子组 | 数量 | 重点 |
| --- | ---: | --- |
| plain bind | 25 | shared/private/slave/unbindable child 与 parent 的组合，shared subtree，bind 到自身子树，多级 slave p-node。 |
| move | 22 | 把 shared/private/slave/unbindable subtree move 到不同 parent，含共享子树、自身绑定树内移动、带不同传播属性 child。 |
| rbind | 40 | recursive bind 复制整棵 mount subtree，覆盖 shared/private/slave/unbindable child 与 subtree 的组合，以及 rbind 到自身子树。 |
| cloneNS | 7 | namespace clone 后 shared/private/slave/unbindable 的跨 namespace 传播和隔离。 |
| regression | 1 | bind/rbind/move unshared directory 到 unshared mountpoint 的基础回归。 |

实现含义：

- `MS_BIND` 需要能克隆一个 mount view。
- `MS_REC | MS_BIND` 需要 clone 整棵 subtree，并按 unbindable 规则跳过或拒绝相关节点。
- `MS_MOVE` 需要移动整棵 attached subtree。
- `MS_SHARED` / `MS_PRIVATE` / `MS_SLAVE` / `MS_UNBINDABLE` 是对 mount 或 subtree 的传播属性修改。
- shared/slave propagation 需要 peer group 和 master/slave 关系，而不是单个 enum flag。
- cloneNS 用例要求 per-task mount namespace；没有它时，全局 namespace 会把隔离语义做错。
- cleanup 依赖 `/proc/mounts`；即使核心语义先实现，procfs mount view 也可能成为 LTP 基础设施 blocker。

## `fs_readonly` / robind 组

`test_robind.sh` 的流程：

1. 格式化并 mount 测试设备到 `dir1`。
2. `mount --bind dir1 dir2-bound`。
3. `mount --bind dir1 dir3-ro`。
4. `mount -o remount,ro,bind dir1 dir3-ro`。
5. 在 readonly bind mount 上运行传入的 growfiles/rwtest/ftest/inode/stream 等命令，期望 readonly enforcement 生效。

这组有 55 个 runtest 条目，单个脚本内部 `TST_TOTAL=3`。它不是只测 bind 成功，而是测：

- 同一底层 filesystem 可以有多个 mount view。
- `remount,ro,bind` 应只改变目标 bind mount 的 per-mount readonly，而不是把所有 sibling mount 或 superblock 全局变 readonly。
- write path、truncate、create、unlink、mmap write 等路径必须统一走 mount readonly enforcement。

对 Anemone 的直接启发：

- 当前 `Mount::ensure_writable()` 是好的入口，但需要保证所有写路径都从当前 `PathRef.mount()` 取 per-mount flags。
- 只把 ext4 superblock 或 block device 改成 readonly 会做错 robind。
- `MS_REMOUNT | MS_BIND | MS_RDONLY` 需要与普通 superblock remount 区分。

## new mount API 组

LTP 涵盖现代 mount API：

- `fsopen01/02`
- `fsmount01/02`
- `open_tree01/02`
- `move_mount01/02`
- `mount_setattr01`

共同路径：

- `fsopen(fs_type, flags)` 创建 filesystem context fd。
- `fsconfig(fd, FSCONFIG_SET_STRING, "source", dev, 0)` 设置 source。
- `fsconfig(fd, FSCONFIG_CMD_CREATE, ...)` 创建 superblock 或 mount context。
- `fsmount(fd, flags, attrs)` 创建 detached mount fd。
- `move_mount(fsmfd, "", AT_FDCWD, MNTPOINT, MOVE_MOUNT_F_EMPTY_PATH)` 把 detached mount attach 到 namespace。
- `open_tree(AT_FDCWD, MNTPOINT, OPEN_TREE_CLONE)` clone 已有 mount tree 为 detached mount fd。
- `mount_setattr(otfd, "", AT_EMPTY_PATH, &attr, sizeof(attr))` 设置 mount attributes，再 `move_mount()` attach。

要求：

- 内核需要 mount fd / detached mount object 表示。
- fd private state 需要承载未 attach mount 或 filesystem context。
- `move_mount()` 是 attach detached mount 和 move attached mount 的统一入口。
- `MOUNT_ATTR_RDONLY`、`NOSUID`、`NODEV`、`NOEXEC`、`NOSYMFOLLOW`、atime attributes 与 legacy mount flags 是同一套内部 mount attributes。
- `fsopen_supported_by_kernel()` 一类 LTP probe 会根据 `ENOSYS` / `EINVAL` / feature support 分类；一旦 syscall 存在但行为不完整，失败会从 TCONF 变成 FAIL。

首批建议不要急于打开这些 syscall 的正向成功，除非 detached mount fd、move attach 和 attr set 都能闭合。

## mount namespace / `pivot_root`

`mountns01` 到 `mountns04` 测试：

- shared mount 在 parent/child namespace 之间传播。
- private mount 不传播。
- slave mount 只接收 master 传播，不向 master 传播。
- unbindable mount 不能被 bind。

`pivot_root01` 测试：

- 需要 `unshare(CLONE_NEWNS | CLONE_FS)`。
- 需要把 `/` 递归改成 private。
- `new_root` 必须是 mountpoint。
- `put_old` 必须位于 `new_root` 下。
- 检查 `EBUSY`、`EINVAL`、`ENOTDIR`、`EPERM` 等失败边界。

这部分应排在基础 bind/remount/move 后面。否则会同时引入 task fs root/cwd、chroot、mount namespace clone、shared mount 限制和 root replacement，评审面过大。

## Linux 6.6 参考分层

Linux UAPI flag 定义位于 Linux 6.6.32 的 `include/uapi/linux/mount.h`：

- legacy mount flags：`MS_RDONLY`、`MS_NOSUID`、`MS_NODEV`、`MS_NOEXEC`、`MS_REMOUNT`、`MS_NOSYMFOLLOW`、`MS_NOATIME`、`MS_NODIRATIME`、`MS_BIND`、`MS_MOVE`、`MS_REC`、`MS_SHARED`、`MS_PRIVATE`、`MS_SLAVE`、`MS_UNBINDABLE` 等。
- unmount flags：`MNT_FORCE`、`MNT_DETACH`、`MNT_EXPIRE`、`UMOUNT_NOFOLLOW`。
- new mount API flags：`OPEN_TREE_*`、`MOVE_MOUNT_*`、`FSOPEN_CLOEXEC`、`FSMOUNT_CLOEXEC`、`MOUNT_ATTR_*`。

Linux 6.6.32 的 `fs/namespace.c` 中 `path_mount()` 先把 flags 拆成 per-mount flags 和 superblock flags，然后按互斥操作分流：

1. `MS_REMOUNT | MS_BIND`：reconfigure mount。
2. `MS_REMOUNT`：remount。
3. `MS_BIND`：loopback / bind，`MS_REC` 控制递归。
4. `MS_SHARED` / `MS_PRIVATE` / `MS_SLAVE` / `MS_UNBINDABLE`：change propagation type。
5. `MS_MOVE`：move mount。
6. 其他：new mount。

这个分流对 Anemone 很重要：`mount(2)` 不是“创建文件系统”一个动作，而是多个 mount namespace transaction 复用同一个 syscall 入口。

Linux `umount` 路径先验证 flag mask，再按 `UMOUNT_NOFOLLOW` 控制 path lookup，要求 target 是 mount root；`MNT_EXPIRE` 使用 mount 的 expiry mark，`MNT_DETACH` 走 lazy detach，普通 unmount 会检查 busy 并同步摘除。

## 建议阶段边界

调查后建议的 staged 顺序：

1. **UAPI / parser / errno gate**：补全 ABI 常量和 flag parser，但只对已闭合操作正向成功；unsupported 操作稳定返回 errno。
2. **mount attributes gate**：扩展 `MountFlags` / `MountAttrs`，让 readonly、noexec、nodev、nosuid、nosymfollow 和 atime 策略有内部表示，并先接 readonly / noexec / nosymfollow 的高价值路径。
3. **plain bind gate**：实现 non-recursive `MS_BIND`，支持绑定目录 subtree view，不 kill source superblock，闭合 `mount05` 和 regression 基础路径。
4. **per-mount readonly remount gate**：实现 `MS_REMOUNT | MS_BIND | MS_RDONLY`，优先支撑 `fs_readonly`。
5. **move gate**：实现 `MS_MOVE` 的 detach/attach，同步检查拓扑环和 busy/propagation 限制。
6. **recursive bind gate**：实现 `MS_REC | MS_BIND` 复制 subtree；先处理 private/unbindable 的简单规则，再扩展 shared/slave。
7. **propagation gate**：引入 shared/private/slave/unbindable、peer group、master/slave 关系和 propagation event。
8. **namespace gate**：把 visible namespace 从全局能力扩展到 task mount namespace，接入 `clone/unshare(CLONE_NEWNS)`。
9. **unmount flags gate**：`MNT_DETACH`、`MNT_EXPIRE`、`UMOUNT_NOFOLLOW`；也可把 `MNT_DETACH` 提前到 bind cleanup 需要时。
10. **new mount API gate**：filesystem context fd、detached mount fd、`open_tree()`、`fsmount()`、`move_mount()` attach、`mount_setattr()`。
11. **pivot_root gate**：在 namespace/root/cwd 基础闭合后实现。

如果短期目标是最大化 LTP 收益，plain bind + per-mount readonly remount + procfs `/proc/mounts` 可观测性可能比完整 shared propagation 更早产生收益。但只要进入 `fs_bind` 全组，propagation 和 namespace 迟早是核心边界，不能在模型里省掉。

## 开放调查点

- 当前所有写路径是否都能拿到正确 `PathRef.mount()` 并调用 `ensure_writable()`。
- `/proc/mounts` 当前输出是否足够让 `fs_bind_lib.sh` cleanup 精确逆序 umount。
- Busy 判断是否应把当前 task/root/cwd、open file、active inode、child mount 分层处理。
- bind mount 的 root dentry 如果不是 superblock root，当前 `Mount` 是否需要新增 `root_path` / `root_dentry` 语义约束。
- ext4 remount readonly 是否只需要 per-mount readonly，还是还要支持 superblock readonly reconfigure。
- `mount -t tmpfs` 可以继续作为 `ramfs` 兼容入口；`ext2` / `ext3` / `vfat -> ramfs` 必须被记录为 syscall-only LTP bridge，并在真实 filesystem support 或测试策略变化后退出。
- LTP runner 当前 profile 是否包含 `fs_bind` / `fs_readonly`，还是需要单独 group 文件隔离运行。
