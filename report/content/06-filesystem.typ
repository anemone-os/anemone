#import "../components/figure.typ": code-block, report-figure

= 文件系统

Anemone 的文件系统围绕 VFS 对象模型展开：上层系统调用、文件描述符表和内存映射只面对统一的 `File`、`Inode`、`Dentry`、`Mount` 和 `PathRef`，具体后端通过静态 operation table 接入。这样，Ext4、ramfs、procfs、devfs、匿名 pipefs 等对象可以共享同一套路由、权限检查、路径查找和打开文件语义。

开发过程中，我们发现文件系统与下一章讲的设备驱动模型都非常依赖对象私有数据。所以，我们为此引入 `Opaque` trait 和 `AnyOpaque` 类型（这里的命名来自Zig的`anyopaque`类型)，提供类型安全的类型擦除能力。VFS 对象只保存 `AnyOpaque`，后端在自己的边界内通过 `cast` 或 `cast_mut` 恢复原始类型；这样既不用把 Ext4、procfs、devfs 或设备状态暴露给通用 VFS，也避免了到处使用裸指针传递私有状态。

#code-block(
  ```rust
  pub trait Opaque: Any + Sync + Send {}

  #[derive(Debug)]
  pub struct AnyOpaque(Box<dyn Opaque>);

  impl AnyOpaque {
      pub fn new<T: Opaque>(data: T) -> Self {
          Self(Box::new(data))
      }

      pub fn cast<T: Opaque>(&self) -> Option<&T> {
          self.0.cast::<T>()
      }

      pub fn cast_mut<T: Opaque>(&mut self) -> Option<&mut T> {
          self.0.cast_mut::<T>()
      }
  }
  ```.text,
  caption: [`Opaque` 与 `AnyOpaque` 提供类型安全的对象私有数据存储],
  lang: "rust",
)

== VFS 对象模型

Anemone 的 VFS 对象采用接近 Linux VFS 的 operation table 形态：通用对象保存静态函数表和后端私有数据，调用时由 VFS 先完成权限、路径、挂载属性和文件光标等公共检查，再把请求转发给具体文件系统或特殊文件对象。这里我们不使用trait，让数据全部集中在一个对象上，更加统一。

=== FileSystemOps

`FileSystemOps` 是文件系统类型的注册入口。每个可挂载文件系统提供名字、属性、挂载函数和同步 / 销毁回调；VFS 根据名字找到 `FileSystem` 后调用 `mount` 生成或复用 `SuperBlock`。

#code-block(
  ```rust
  pub struct FileSystemOps {
      pub name: &'static str,
      pub flags: FileSystemFlags,
      pub mount: fn(MountSource, MountData) -> Result<Arc<SuperBlock>, SysError>,
      pub sync_fs: fn(&SuperBlock) -> Result<(), SysError>,
      pub kill_sb: fn(Arc<SuperBlock>),
  }
  ```.text,
  caption: [`FileSystemOps` 是文件系统类型接入 VFS 的入口],
  lang: "rust",
)

=== SuperBlock

`SuperBlock` 表示一个已经挂载的文件系统实例。对应的 `SuperBlockOps` 负责按 inode number 载入 inode、在 resident inode cache 回收时写回或取消回收，以及同步 inode 元数据。Ext4、ramfs、procfs 等后端都把自己的`SuperBlock`私有状态放在 `AnyOpaque` 中，通用层只维护 superblock、mount 和 inode cache 的关系。

#code-block(
  ```rust
  pub(super) struct SuperBlockOps {
      pub load_inode:
          fn(&Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, SysError>,
      pub evict_inode: fn(Arc<Inode>) -> Result<(), SysError>,
      pub sync_inode: fn(&InodeRef) -> Result<(), SysError>,
  }
  ```.text,
  caption: [`SuperBlockOps` 负责 inode 载入、回收和元数据同步],
  lang: "rust",
)

=== Mount

`Mount` 是挂载树中的视图对象。同一个 `SuperBlock` 可以通过多个 `Mount` 出现在不同路径下，bind mount 也通过创建新的 `Mount` 视图来共享原有 superblock 和 dentry 树。只读属性保存在 mount 视图上，因此同一后端可以在不同挂载点呈现不同的写入策略。

#code-block(
  ```rust
  pub struct Mount {
      root: Arc<Dentry>,
      sb: Arc<SuperBlock>,
      placement: SpinLock<MountPlacement>,
      children: SpinLock<Vec<Weak<Mount>>>,
      attrs: AtomicU32,
  }
  ```.text,
  caption: [`Mount` 保存一次挂载视图的根目录、后端 superblock、位置和属性],
  lang: "rust",
)

可以注意到这里的`attrs`是原子字段，这是为了保证挂载点属性修改的原子事务，以免用户态观测到竞态窗口。

#report-figure(
  image("../assets/vfs.png", width: 90%),
  caption: [FileSystem，SuperBlock，Mount，层层叠加的关系],
)

=== Inode

`Inode` 是文件系统对象和元数据的核心抽象。VFS 在调用 `InodeOps` 前处理路径权限、挂载可写性和通用错误语义；后端则实现目录查找、创建、链接、删除、重命名、打开、截断、符号链接读取和属性查询等真正依赖文件系统格式的操作。

#code-block(
  ```rust
  pub struct InodeOps {
      pub lookup: fn(&InodeRef, &str) -> Result<InodeRef, SysError>,
      pub touch: fn(&InodeRef, &str, InodePerm) -> Result<InodeRef, SysError>,
      pub mkdir: fn(&InodeRef, &str, InodePerm) -> Result<InodeRef, SysError>,
      pub symlink: fn(&InodeRef, &str, &Path) -> Result<InodeRef, SysError>,
      pub link: fn(&InodeRef, &str, &InodeRef) -> Result<(), SysError>,
      pub unlink: fn(&InodeRef, &str) -> Result<(), SysError>,
      pub rmdir: fn(&InodeRef, &str) -> Result<(), SysError>,
      pub rename:
          fn(&InodeRef, &str, &InodeRef, &str, RenameFlags) -> Result<(), SysError>,
      pub open: fn(&InodeRef) -> Result<OpenedFile, SysError>,
      pub truncate: fn(&InodeRef, u64) -> Result<(), SysError>,
      pub read_link: fn(&InodeRef) -> Result<PathBuf, SysError>,
      pub get_attr: fn(&InodeRef) -> Result<InodeStat, SysError>,
  }
  ```.text,
  caption: [`InodeOps` 覆盖目录、文件、链接和元数据操作],
  lang: "rust",
)

我们还在`SuperBlock`中维护了一个icache，这样，已经被加载的Inode可以缓存在内存中，如果后续再次命中，就可以大大提高系统效率。而同时，我们还引入了Inode Shinker内核线程。当内核内存占用过多时，shrinker会遍历各个超级快，尝试驱逐*引用计数已经归零*的Inode。

#report-figure(
  image("../assets/inode-lifecycle.png", width: 90%),
  caption: [一个Inode的生命周期],
)

=== Dentry

`Dentry` 保存“名字到 inode”的路径结构。目录 dentry 会维护子项弱引用缓存，路径查找先尝试命中已有 dentry，未命中时再通过父目录的 `InodeOps::lookup` 向后端请求 inode 并创建新的 dentry。它不记录 mount 关系；跨挂载点的位置被 `PathRef { mount, dentry }` 共同表达。

#code-block(
  ```rust
  pub struct Dentry {
      parent: Option<Arc<Dentry>>,
      inode: InodeRef,
      inner: RwLock<DentryInner>,
  }

  struct DentryInner {
      name: String,
      children: Option<HashMap<String, Weak<Dentry>>>,
  }
  ```.text,
  caption: [`Dentry` 保存路径树节点和目录子项缓存],
  lang: "rust",
)

=== File

`File` 表示一次打开后的文件对象，它保存打开时的 `PathRef`、文件操作表、文件模式、后端私有数据和文件光标。普通文件读写使用 VFS 管理的光标；Anemone还支持stream 类对象，允许后端文件系统可以在打开时标记自己的文件模式，让后端自行维护读写位置。

#code-block(
  ```rust
  pub struct FileOps {
      pub read:
          fn(&File, pos: &mut usize, buf: &mut [u8], FileIoCtx)
              -> Result<usize, SysError>,
      pub write:
          fn(&File, pos: &mut usize, buf: &[u8], FileIoCtx)
              -> Result<usize, SysError>,
      pub read_at:
          fn(&File, pos: usize, buf: &mut [u8], FileIoCtx)
              -> Result<usize, SysError>,
      pub write_at:
          fn(&File, pos: usize, buf: &[u8], FileIoCtx)
              -> Result<usize, SysError>,
      pub check_status_flags:
          fn(&File, FileOpStatusFlags) -> Result<(), SysError>,
      pub seek: fn(&File, pos: &mut usize, SeekFrom) -> Result<usize, SysError>,
      pub read_dir:
          fn(&File, pos: &mut usize, &mut dyn DirSink)
              -> Result<ReadDirResult, SysError>,
      pub poll:
          for<'a> fn(&File, &PollRequest<'a>)
              -> Result<PollRegisterResult, SysError>,
      pub fcntl: Option<FileFcntlHook>,
      pub ioctl: for<'a> fn(&File, IoctlCtx<'a>) -> Result<u64, SysError>,
  }
  ```.text,
  caption: [`FileOps` 是打开文件对象的统一操作表],
  lang: "rust",
)

== 挂载树

Anemone 用全局 VFS 子系统维护可见挂载树和匿名挂载树。可见挂载树承载根文件系统、用户可见的磁盘文件系统、ramfs、procfs 和 devfs；匿名挂载树用于 pipe、eventfd、timerfd 等不需要路径查找的内核内部文件对象。路径查找后，得到 `PathRef`，其中同时包含当前挂载点和 dentry，因此同一个 inode 经由不同 bind mount 被访问时仍能保留路径视图差异。

#report-figure(
  image("../assets/path.png", width: 70%),
  caption: [路径查询的例子],
)

挂载操作集中由 `MountTree` 处理。它支持根挂载、普通挂载、bind / recursive bind、move mount、remount 属性更新、private propagation 请求、普通卸载和 lazy detach。挂载树是 mount 位置的唯一写入者，`Dentry` 不反向记录挂载关系；路径查找遇到挂载点时查询当前 mount stack 的最上层视图。如果查找过程中挂载树发生变化，VFS 通过 placement generation 重试，避免返回过期路径视图。这样，我们就避免用户看到过期状态。

== VMO 形式的页缓存

Ext4 普通文件的缓存直接复用内存管理章节介绍过的 VMO 机制。每个 Ext4 regular inode 的私有状态中维护按页编号索引的缓存页，`Ext4RegMapping` 实现 `VmObject`：普通 `read` / `write` 通过同一个 mapping 复制数据，文件 `mmap` 的缺页路径也通过 `resolve_frame` 取得同一批物理页。

举个例子。读路径第一次访问某页时，从硬盘读取文件内容并填入新分配的 frame；后续读或映射访问就能直接命中缓存。而写路径会在必要时先加载旧页，修改 frame 后把页标记为 dirty。`sync_range`、`sync_all` 和文件系统同步路径再把脏页写回 Ext4 后端。文件截断时，Anemone 会使可见范围发生变化的缓存页失效，下一次访问再从后端重新载入，避免旧页内容跨越新的文件大小边界。

== Ext4 支持

当前，Anemone已经通过引入lwext4这个经典的C库，为自身接入了Ext4支持，从而为用户态兼容提供强大的保障。
