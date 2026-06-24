#import "../template/components.typ": *
#import "../template/figures.typ": *

= 设备驱动模型与 I/O 对象

#epigraph(attribution: [Butler Lampson, @lampson1983hints])[
  An interface should capture the minimum essentials of an abstraction. Don't generalize; generalizations are generally wrong.
]

#thesis[
  Anemone 的设备模型独立于 VFS。VFS 可以通过 devfs、`FileOps` 和 `ioctl` 把设备暴露成用户态熟悉的文件对象，但设备身份、I/O 语义、私有控制面和生命周期仍由 device / driver owner 维护。devfs 是 namespace bridge，`FileOps` 是打开文件后的窄接口，`ioctl` 是穿过 VFS 的控制请求；解释权不能停在 VFS 层。
]

Unix 风格接口让设备看起来像文件，这是一种强大的用户态约定，也是一种容易误导内核结构的比喻。如果把“像文件”理解成“由 VFS 拥有设备语义”，设备路径很快会长出两个问题：一是 VFS 会被迫理解每个 driver 的私有命令；二是设备状态会在 driver registry、devfs inode、opened file object 和 ioctl helper 之间出现多份真相。Anemone 的路线是保留文件接口，但把设备 owner 留在设备子系统。

== Device / Driver / Bus

Anemone 的 device / driver / bus 形状先于 `/dev` 路径存在。device 是已经被 firmware、PCIe config space、virtio transport 或内核自身构造出来的对象；driver 是能识别这类对象并建立 concrete owner 的代码；bus 则拥有 discovery 之后的 matching、probe 顺序和 bus-specific resource protocol。`DeviceBase` 记录 firmware node、children、已绑定 driver 和 driver-private state，`DriverBase` 记录已经 attach 的 device，`BusTypeBase` 则保存同一类 bus 上的 devices 和 drivers。

#design-note[
  `drv_state` 没有退回 C 风格的裸 `void *`。Anemone 的 `Opaque` trait 约束私有数据必须同时满足 `Any + Send + Sync`，`AnyOpaque` 再用 type-erased box 保存它；读取方必须按具体类型 downcast，失败会显式返回 `None`。这让 driver-private state 保留了“由具体 driver 解释”的边界，同时把任意指针强转收束成可审查的 Rust 类型擦除。
]

`BusType::register_device()` 和 `BusType::register_driver()` 都会尝试从另一侧集合里寻找 match：platform bus 比较 device tree compatible string，virtio bus 比较 virtio device id，PCIe bus 先看 vendor/device id，再退到 class code。match 成功之后，bus 调用 driver 的 `probe()`；PCIe bus 还把 preinit、BAR/resource allocation、postinit 纳入同一条 probe sequence。probe 成功才会把 driver 写入 device，并把 device attach 到 driver。这个绑定关系仍在设备模型内部，不需要 `/dev` 节点参与。

#listing([bus 拥有 matching 和 probe，driver 才建立 concrete owner])[
```rust
pub trait Device: DeviceData + DeviceOps {
    fn driver(&self) -> Option<Arc<dyn Driver>>;
    fn set_driver(&self, driver: Option<Arc<dyn Driver>>);
    fn set_drv_state(&self, state: AnyOpaque);
}

pub trait DriverOps {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError>;
    fn shutdown(&self, device: &dyn Device);
}

pub trait BusType: KObject {
    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool;
    fn register_device(&self, device: Arc<dyn Device>);
    fn register_driver(&self, driver: Arc<dyn Driver>);
}
```
]

文件路径只是设备模型晚到的一层发布结果。devfs 只回答“用户态路径如何打开这个 I/O object”；device / driver / bus 回答的是“内核如何知道这个对象存在、哪个 driver 能接管它、接管时需要哪些 bus-specific 资源”。有些 driver probe 后会创建字符设备或块设备，有些只建立 clock source、interrupt controller、transport 或 power owner。不是每个 device 都需要 `/dev` 节点，也不是每个设备 owner 都应该通过文件接口暴露完整控制面。

#book-figure(
  "../assets/figures/ch06/device-driver-bus.png",
  [bus owns matching and probe; driver creates the I/O owner.],
  width: 100%,
)

== Probe path 与 I/O publication

probe 之后进入 I/O class registry 的只是设备模型的一部分。字符设备通过 `register_char_device()` 把 `CharDevNum`、canonical name 和 `Arc<dyn CharDev>` 放入 char subsystem registry；块设备通过 `register_block_device()` 把 `BlockDevNum`、`BlockDevClass`、自动分配的设备名和 `Arc<dyn BlockDev>` 放入 block registry。

这一层的 owner 仍是设备子系统。字符设备决定字节流读写、seek 和 private ioctl；块设备决定 block size、capacity、block I/O、通用 block metadata 与 private ioctl；loop 作为 block device class，把普通文件 backing 适配成 block device；virtio-blk、ramdisk 等设备则不需要理解 VFS fd table。VFS 可以拿到一个 `DeviceId::Char` 或 `DeviceId::Block`，但这只是定位到设备 owner 的路由信息。

#listing([char / block trait 拥有设备语义，devfs 只负责把它们暴露成节点])[
```rust
pub trait CharDev: Send + Sync {
    fn devnum(&self) -> CharDevNum;
    fn read(&self, buf: &mut [u8]) -> Result<usize, SysError>;
    fn write(&self, buf: &[u8]) -> Result<usize, SysError>;
    fn seek(&self, _ctx: CharSeekCtx<'_>) -> Result<usize, SysError> { ... }
    fn ioctl(&self, _ctx: CharIoctlCtx<'_>) -> Result<u64, SysError> { ... }
}

pub trait BlockDev: Send + Sync {
    fn devnum(&self) -> BlockDevNum;
    fn block_size(&self) -> BlockSize;
    fn total_blocks(&self) -> usize;
    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError>;
    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError>;
    fn ioctl(&self, _ctx: BlockIoctlCtx<'_>) -> Result<u64, SysError> { ... }
}
```
]

== Devfs publish layer

devfs 是 global singleton `/dev` publish layer。它有一个静态 publish-only registry：每个 `DevfsPublish` 包含 name、inode 属性、`rdev` 和稳定长生命周期的 `DevfsNodeOps`。publish 时 devfs 分配 stable inode number，seed singleton superblock icache，然后把名字放进 registry。lookup 时 devfs 只解析名字并返回已经存在的 inode；open leaf 时，devfs 调用发布方的 `DevfsNodeOps::open()`，由设备侧返回真实 `OpenedFile`。

这条路径在源码注释里写得很直接：devfs 只拥有 name lookup 和 stable inode identity，device-attached semantics 应留在 owning subsystem。字符设备的 `publish_char_device()` 会把 `CharDevNum` 发布为 `InodeType::Char` 节点，open 后使用统一 `CHAR_DEV_FILE_OPS`，而该 file ops 会通过 `rdev` 回到 char registry 查找真实 `CharDev`。块设备的 `publish_block_device()` 同样发布 `InodeType::Block` 节点，open 后使用统一 `BLOCK_DEV_FILE_OPS`，通用 `BLK*` 命令和私有 block ioctl 都从这里路由回 block owner。

#book-figure(
  "../assets/figures/ch06/devfs-device-bridge.png",
  [devfs 暴露设备节点，但设备语义仍由 driver owner 决定。],
  width: 100%,
)

devfs 的 trade-off 也来自这个选择。启动期静态 publish 到扁平 `/dev` 根目录，和为运行环境保留静态 `/dev/shm` 目录挂载点，证明的是 namespace bridge 与 stable inode identity 已经成立；通用目录层级、hot-unplug、unpublish、别名和 symlink 协议仍需要独立的设备生命周期 contract。换句话说，节点可见性和设备生态完整性是两层不同承诺。

== `FileOps` device bridge

设备最终还是要通过文件接口服务用户态，所以 VFS 与设备 owner 之间需要一层 narrow file-object API。字符设备的统一 file ops 把 `read`、`write`、`seek` 和 `ioctl` 都转成对 `CharDev` trait 的调用；块设备的统一 file ops 负责 byte-oriented block device read/write、seek、通用 `BLKGETSIZE64` / `BLKGETSIZE` / `BLKSSZGET` / readahead ioctl，再把未匹配命令交给 `BlockDev::ioctl(BlockIoctlCtx)`。

这个分工不是为了少写代码，而是为了防止 `/dev/loopN`、`/dev/ram0`、`/dev/vda` 等设备各自发布一套绕过通用 block behavior 的 file ops。loop 是一个很好的反例约束：它注册为真实 `BlockDevClass::Loop` 的 block device，通过统一 block devfs file ops 接收 read/write、`BLK*` 和 `LOOP_*`；它不能为 `/dev/loopN` 发明一套专属 file ops，也不能让 mount 层直接理解普通 image 文件或 `-o loop`。

#boundary[
  `FileOps` 是 VFS 与设备 owner 之间的窄接口，不是设备模型的父类系统。它接收打开后的 `File` 和本次操作 context；设备身份、I/O 状态机和私有控制协议仍回到 char / block / concrete driver owner。
]

== `ioctl` ownership model

`ioctl` 是本章最容易混乱的路径。系统调用入口需要理解 fd、用户指针、`O_PATH`、目标访问能力和 fd-argument lookup；但它不能理解所有设备私有命令。Anemone 的 `sys_ioctl()` 做最外层 ABI 工作：查 fd，生成 `IoctlFileAccess` snapshot，拒绝 path-only 目标，捕获当前用户地址空间 handle，构造受控的 fd 参数 lookup helper，然后把 `IoctlCtx` 交给打开文件的 `FileOps::ioctl`。

`IoctlCtx` 的价值在于它足够窄。它携带 `cmd`、`arg`、目标 fd 的访问能力快照、用户空间访问 handle 和受控 arg-fd lookup；不携带 `FileDesc`、`ProcFile`、`FilesState`、完整 task 或 fd table。字符设备再把它包成 `CharIoctlCtx`，块设备再把它包成 `BlockIoctlCtx` 并附带 block I/O 序列化能力。设备可以按命令语义知道 `arg` 是用户指针或 fd number，但 fd lookup 只能通过 helper 完成；成功后如果需要延长生命周期，也要转成 owner 自己的 handle。

#listing([`IoctlCtx` 把 fd/table/task 状态压缩成一次 ioctl 请求可以使用的能力快照])[
```rust
pub struct IoctlCtx<'a> {
    cmd: u32,
    arg: u64,
    target_access: IoctlFileAccess,
    uspace: Arc<UserSpaceHandle>,
    arg_fd_lookup: &'a IoctlArgFdLookup,
}

pub struct CharIoctlCtx<'a> {
    inner: IoctlCtx<'a>,
}

pub struct BlockIoctlCtx<'a> {
    inner: IoctlCtx<'a>,
    io: BlockDevIoHandle,
}
```
]

loop 的 `LOOP_SET_FD` 说明了为什么这条边界重要。用户传入的 `arg` 最初只是调用者 fd table 中的一个 raw fd number；设备不能把这个 number 保存成长期状态，也不能保存 `FileDesc` 后以后再推断访问能力。Anemone 在 VFS/fd 边界把它转成 `BackingFileHandle`：它验证目标不是 path-only、具备 read access、是 regular file，并记录是否可写、display name 和 `Arc<File>`。loop state 保存这个 narrowed handle，再把 file-backed positioned I/O 转成 block device I/O。

#book-figure(
  "../assets/figures/ch06/ioctl-owner-boundary.png",
  [ioctl 控制面穿过 VFS，但最终由设备 owner 解释。],
  width: 100%,
)

== Memory char device、block helper 与 loop

`/dev/null`、`/dev/zero` 和 `/dev/full` 展示了字符设备的最小 owner policy。它们在各自 `CharDev` 实现中显式 override null-style seek，把 position 设为 `0` 并返回 `0`；`/dev/urandom` 没有 override 时就走默认不可 seek。这个行为不在 devfs 或 VFS 中硬编码，也不要求所有 char device 都继承同一种 seek 语义。

块设备则展示了另一种统一 helper。默认 block devfs 路径提供 byte-oriented read/write 和通用 `BLK*` 查询，但真实 I/O 仍通过 `BlockDev::read_blocks` / `write_blocks` 执行。byte I/O helper 能处理非块对齐 offset，必要时使用 bounce buffer；这提高了 Linux-visible 兼容性。它当前承诺的是通用 block file path 和查询能力，不是完整 Linux block device file compatibility layer；waitable poll、partscan、sysfs 可观察面和更细的 block policy 仍属于 block owner 的后续 contract。

loop 设备把 VFS-backed file 和 block device 连接起来，是一个有意收窄的例外。它可以持有 `BackingFileHandle`，因为 loop 的定义就是普通文件到块设备的适配；但这个依赖不能推广到 virtio、ramdisk 或普通物理块设备。mount 层也不因为 loop 存在就理解普通 image 文件；用户态 mount 工具负责把 `mount -o loop file.img` 转成 loop ioctl + 普通 block source mount。

== TradeOff: VFS bridge 与设备 owner contract

设备模型最重要的边界不是“还缺哪些节点”，而是哪些 owner contract 已经能被清楚表达。devfs publish record、block byte I/O helper、char seek/ioctl hook 和 loop backing handle 说明桥接形状已经存在；具体设备协议、sysfs control surface、Linux block / tty / network 语义则属于更外层的设备生态承诺。一个设备节点能被打开，只表示 namespace bridge 成立，不表示设备生态已经完整。

这条线也解释了为什么 Anemone 不把所有设备能力都塞进 `ioctl` 或 devfs。文件接口是用户态兼容面，driver owner 才是设备语义的来源。VFS 让对象可被打开、读写、poll、seek 和控制；设备状态、生命周期和私有协议仍留在 driver owner 的 contract 内。
