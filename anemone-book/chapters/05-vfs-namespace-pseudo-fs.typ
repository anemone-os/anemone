#import "../template/components.typ": *
#import "../template/figures.typ": *

= VFS、命名空间与 Pseudo Filesystems

#epigraph(attribution: [David L. Parnas, @parnas1972criteria])[
  We propose instead that one begins with a list of difficult design decisions or design decisions which are likely to change.
]

#thesis[
  VFS 在 Anemone 中不是“所有文件系统功能的总管”，而是一组把名字、挂载视图、opened file description、`File`、`Inode` 和 filesystem backend 连接起来的对象边界。procfs、devfs 以及未来可能出现的 sysfs 更适合被理解为 namespace bridge / control surface：它们把 task、mount、device、sysctl 等内核对象变成路径、目录和文件，但不因此取得这些对象的核心状态所有权。
]

文件系统一旦被放进 `/` 下面，就很容易被误读成“路径拥有对象”。Anemone 刻意避免这个方向。路径是可见性和定位协议，`File` 是一次 open 后的内核对象，`FileDesc` 是 fd table 发布出去的 opened file description，`Inode` 是 filesystem backend 暴露的对象身份，`Mount` 则是某个 superblock 在挂载树中的 view。这些对象通过 VFS 相遇，但它们的状态 owner 并不相同。

== 文件不是 fd

用户态看到的是 fd number，但内核里真正共享的是 opened file description。`FileDesc` 包装共享的 `ProcFile`，其中包含 task-agnostic 的 `Arc<File>`、访问模式、opened-description status flags、Linux 兼容可见位和 final-release hook；fd-local 的 `FdFlags` 则留在 descriptor 层。dup 和 fork 后的 fd 可以共享同一个 opened file description，因此 `O_NONBLOCK`、`O_APPEND` 这类 file status flags 不能偷偷复制到 pipe、block device 或 procfs 节点内部，必须由 fd/files 层作为单一真相源维护，再以短生命周期 context 传给 `FileOps`。

`File` 代表已经打开的文件对象。它持有 `PathRef`、`FileOps`、不可变的 open-time `FileMode`、backend private data 和 VFS-managed cursor。普通 `read` / `write` 会使用本地 cursor；stream file 可以把 cursor 留给 backend；positioned I/O 通过 `read_at` / `write_at` 走单独 hook，不再用“先 seek 再 read/write”的兼容 wrapper 偷换语义。这样的分层让 fd-local policy、VFS-wide gate 和 backend capability 各自有边界。

#listing([`FileOps` 摘录：打开文件对象的窄接口不接收 fd table 或 Linux raw UAPI])[
```rust
pub struct FileOps {
    pub read: fn(&File, pos: &mut usize, buf: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub write: fn(&File, pos: &mut usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub read_at: fn(&File, pos: usize, buf: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub write_at: fn(&File, pos: usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub check_status_flags: fn(&File, FileOpStatusFlags) -> Result<(), SysError>,
    pub seek: fn(&File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError>,
    pub ioctl: for<'a> fn(&File, IoctlCtx<'a>) -> Result<u64, SysError>,
}
```
]

这里的重点不是 vtable 长什么样，而是它没有接收 `FileDesc`、当前 task 或 fd table。backend 只看到一个打开后的 `File` 和本次操作需要的 context：I/O status flags 是 snapshot，fcntl 只暴露 file-object 子集，ioctl 也会被压缩成 `IoctlCtx`。fd-local policy 和 Linux syscall adapter 语义留在边界层，filesystem backend 的长期状态只承载 backend 自己拥有的对象语义。

#design-note[
  Anemone 在 VFS 里选择显式 `FileOps` vtable，而不是把这层边界写成一个 Rust trait。这个选择并不是因为 trait 无法表达同样的 dispatch，也不是要证明 vtable 一定更正确；它首先是一种工程偏好：operation table 作为一份普通数据，可以被静态初始化、局部检查和按字段阅读，后端支持哪些操作、哪些 hook 是默认错误、哪些 context 被允许穿过边界，都比一个分散的 trait impl 更直观。

  这个口味显然受 Linux VFS `file_operations` 影响。Anemone 借用了那种“打开文件对象携带一张操作表”的直觉，但没有把 Linux 的完整 VFS 结构搬进 Rust；`FileOps` 仍只接收 `File` 和短生命周期 context，fd table、task、raw UAPI 和设备私有控制面都留在各自 owner 附近。换句话说，这是向 Linux 取一段透明的接口形状，而不是把 Linux 内部组织作为目标。
]

== `PathRef = Mount + Dentry`

Anemone 的路径位置由 `PathRef` 表达：一个 `Arc<Mount>` 加一个 `Arc<Dentry>`。这个形状和 Linux `struct path` 的核心直觉相似，但更重要的是 owner boundary：`Dentry` 表达 filesystem tree 中的名字边，`Mount` 表达这个 tree 在 mount tree 里的 view。单独一个 dentry 不足以说明“从哪个 namespace view 看到它”，单独一个 mount 也不能说明“指向这个 filesystem instance 的哪一个节点”。

#listing([`PathRef` 把 mount view 与 dentry 放在同一个位置对象里])[
```rust
pub struct PathRef {
    mount: Arc<Mount>,
    dentry: Arc<Dentry>,
}

impl PathRef {
    pub fn inode(&self) -> &InodeRef {
        &self.dentry.inode()
    }

    pub fn open(&self) -> Result<File, SysError> {
        let OpenedFile { file_ops, mode, prv } = self.inode().open()?;
        Ok(File::new_with_mode(self.clone(), file_ops, mode, prv))
    }
}
```
]

`MountTree` 是 topology 的写侧 owner。attach、detach、bind、recursive bind、move、remount 和 lazy detach 都要通过 `MountTree` transaction；`Mount` 上保存 placement cache，是为了让旧 `PathRef` 和诊断仍能理解对象状态，不是为了给普通 VFS 代码提供第二条拓扑修改路径。readonly remount 选择 per-mount view attribute：`Mount` 上的 atomic attrs 是只读挂载 enforcement 的单一真相源，写路径通过 live `PathRef.mount()` acquire-load 当前属性，而不是把 readonly 写入 superblock 影响 sibling bind mount。

#book-figure(
  "../assets/figures/ch05/vfs-object-model.png",
  [`FdTable`、`File`、`Inode` 与 `Mount` 属于不同 ownership 层，不应彼此缓存对方状态。],
  width: 100%,
)

== Mount view 与可见性

bind mount 的设计最能说明 mount view 的意义。一次 bind 不复制 superblock，也不复制被挂载目录下的 filesystem object；它创建新的 `Mount` view，复用 source 的 superblock，并把某个 source dentry 作为这个 view 的 root。recursive bind 递归克隆的是 mount views，而不是把底层文件系统树重新复制一份。move mount 移动的是同一个 mount subtree 的 placement，成功后同一个 `Arc<Mount>` identity 从新位置可见。

这也是为什么 mount tree 设计把 `NameSpace` 正名为 `MountTree`，并明确它不是完整 Linux mount namespace。visible / anonymous 两棵树提供 mount topology owner 和查找视图；per-task namespace、`nsproxy`、namespace fd 等能力需要自己的 owner。它是 namespace bridge，是因为它把对象接入路径可见性，不是因为 Anemone 已经实现完整 Linux namespace 子系统。

`/proc/<tgid>/mounts` 是这个模型的一个观察面。它从当前 visible mount snapshot 生成六列 live view，并按读取 task 的 root 做路径渲染；`/proc/mounts` 则作为 `self/mounts` symlink 复用同一入口。这个文件不是 mount tree 的 owner，它只是把 `MountTree` 的当前 view 按 Linux 用户态能消费的格式渲染出来。

#book-figure(
  "../assets/figures/ch05/mount-view-visibility.png",
  [mount view 决定路径可见性，而不是复制 filesystem object。],
  width: 100%,
)

== Filesystem backend ownership

VFS 的另一条边界是 `InodeOps` / `FileOps` 与具体 filesystem backend。ramfs、ext4、devfs、procfs、anonymous fs 都可以在自己的 inode/file ops 中解释 lookup、open、read、write、truncate、symlink、directory enumeration 等行为；VFS 负责把路径、权限、mount readonly gate、fd status snapshot 和 syscall adapter 的结果组织成调用。这样做的代价是边界层要更明确：create-open-unlink、path-only fd、动态 file status flag、readonly view 与 mmap/writeback 的关系，都必须作为 adapter trade-off 留在边界附近，而不能被 backend 默默消化成“差不多能跑”。

这个选择让“文件系统”不再是一个单一抽象。VFS 关心路径、打开、fd 发布、operation dispatch 和跨 backend 的 common gate；ext4 关心磁盘 inode、block I/O 和 page cache；ramfs 关心内存 inode 和 lazy allocation；procfs 关心 synthetic inode 和 read snapshot；devfs 关心设备节点的名字桥。它们共享 VFS 的对象协议，但不共享一份隐藏的全局状态。

== Procfs namespace bridge

procfs 是最容易越界的 pseudo filesystem，因为它既像目录树，又像系统控制面。Anemone 的 procfs 由 singleton superblock 承载，所有 procfs mounts 复用同一个 superblock。静态部分使用 `ProcDirEntry` 描述 `/proc` 根下的 PDE tree；动态部分为 `/proc/<tgid>` 和子 inode 按 thread group 绑定生成。task topology 拥有 thread group 的生命周期，procfs 只在 topology transaction 调用的 hook 中失效 binding 和 inode index。

`/proc/<tgid>/fd` 的实现展示了这条边界。fd 目录枚举通过 task 的 fd number snapshot 观察当前打开集合；动态 `fd/<n>` inode 只保存 `(ThreadGroupBinding, Fd)`，不长期保存 `Arc<FileDesc>`。`readlink()`、`getattr()` 和未来的 open 都必须按操作重新验证 binding alive、same-tgid access 和当前 fd 是否仍打开。child inode cache 只表达 procfs synthetic identity，不能证明 fd 还活着。

`/proc/sys/kernel` 这类 sysctl 节点也是类似模式。PDE tree 表达名字、拓扑、inode identity 和通用短文件分发；具体内容来自 owning subsystem 的常量或 helper。只读观察面不等于完整 sysctl transaction，也不等于相关 subsystem 的控制协议已经闭合。

#boundary[
  pseudo filesystem 可以是 control surface，但 control 不等于 ownership。procfs 可以触发读写操作、格式化快照或把请求分发到 owner；只要 task、mount、sysctl、device 或 mm 的核心状态仍由原 subsystem 拥有，procfs 就只是桥。
]

== Devfs publish layer

devfs 也挂在 VFS 之下，但它不是设备模型本身。devfs 是 singleton `/dev` publish layer：它保存稳定 inode number、名字、`rdev` 和 `DevfsNodeOps`；lookup 解析名字并返回已 seed 的 inode；open leaf 时回调发布方的 ops。字符设备和块设备的默认 `/dev` 行为分别由 char / block subsystem 拥有，devfs 只保存 publish record 并分发。

pseudo filesystem 把对象放到 namespace 里，并提供观察或控制入口；task、mount、device、sysctl 或 memory object 的核心状态仍回到原 owner。devfs 的设备语义因此仍回到 device owner，VFS 只承载 publish record、路径解析和打开分发。

== TradeOff: 对象分层与 Linux-visible 缺口

这套设计的 trade-off 是，边界必须比功能清单更清楚。mount propagation、完整 mount namespace、path-only fd、text-busy accounting、truncate / mmap coherency、procfs magic-link 与跨进程权限，都不能由 VFS / pseudo filesystem 边界自然消除；它们需要 mount、task、memory、device 或 filesystem backend owner 分别收敛。它们共同指向同一条线：VFS 和 pseudo filesystem 负责把对象接入 namespace，不替这些 owner 拥有真实状态。

这些限制不是 VFS 设计失败，而是边界成本。Anemone 选择先让 owner boundary 清楚，再逐步补 Linux-visible surface。路径和 pseudo fs 是对象进入 namespace 的方式；对象状态仍回到拥有它的 subsystem。
