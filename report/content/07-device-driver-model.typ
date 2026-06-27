#import "../components/figure.typ": code-block

= 设备驱动模型

Anemone 的设备驱动模型参考了 Linux 的 device / driver / bus 分层，但没有把设备发现、驱动绑定和用户态文件接口混在 VFS 内部。设备模型负责发现硬件、匹配驱动、保存驱动私有状态；VFS 只处理路径、inode 和 opened file object；`devfs` 作为中间桥，把已经注册的字符设备和块设备发布为 `/dev` 下的节点。这样，设备驱动模型与 VFS 对彼此的内部状态保持解耦，设备语义仍由对应的 driver owner 决定。

初赛测例中，许多 I/O 相关测例会同时经过路径查找、设备节点、file operation、ioctl 和后端驱动。如果这些职责直接堆在 open 或 ioctl 的 syscall 层，后续增加块设备、串口、随机数设备、loop 设备和 PCIe 设备时很容易变成特判集合，或者大幅污染内核代码的边界，导致后续维护很困难。

为了解决这种问题，我们把“设备如何被发现和绑定”与“设备如何以文件形式暴露”分开，使同一套驱动模型可以服务 RISC-V、LoongArch、VirtIO MMIO、VirtIO PCIe 等不同平台和传输方式。

== 核心对象

设备驱动模型的核心对象是 `BusType`、`Device` 和 `Driver`。`BusType` 保存本总线上的设备和驱动集合，并定义匹配规则；`Device` 表示一个已经发现的设备节点，保存父子拓扑、固件节点和 driver 私有状态；`Driver` 提供 probe、shutdown 等操作，并记录自己已经绑定的设备。

#code-block(
  ```rust
  pub trait BusType: KObject {
      fn base(&self) -> &BusTypeBase;
      fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool;
      fn register_device(&self, device: Arc<dyn Device>);
      fn register_driver(&self, driver: Arc<dyn Driver>);
  }

  pub trait DeviceData: KObject {
      fn base(&self) -> &DeviceBase;
  }
  pub trait DeviceOps {}
  pub trait Device: DeviceData + DeviceOps {
      fn driver(&self) -> Option<Arc<dyn Driver>>;
      fn set_driver(&self, driver: Option<Arc<dyn Driver>>);
      fn set_drv_state(&self, state: AnyOpaque);
      fn add_child(&self, child: Arc<dyn Device>);
      fn fwnode(&self) -> Option<&Arc<dyn FwNode>>;
  }

  pub trait DriverData: KObject {
      fn base(&self) -> &DriverBase;
  }
  pub trait DriverOps {
      fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError>;
      fn shutdown(&self, device: &dyn Device);
      fn as_platform_driver(&self) -> Option<&dyn PlatformDriver>;
      fn as_virtio_driver(&self) -> Option<&dyn VirtIODriver>;
      fn as_pcie_driver(&self) -> Option<&dyn PcieDriver>;
  }
  pub trait Driver: DriverData + DriverOps {
      fn attach_device(&self, device: Arc<dyn Device>);
  }
  ```.text,
  caption: [`BusType`、`Device` 和 `Driver` 是设备发现、匹配和绑定的核心接口],
  lang: "rust",
)

这三个对象的关系决定了 Anemone 的 probe 流程。内核启动时，会先注册内置驱动，随后解析固件描述并创建设备对象；总线在注册设备或注册驱动时都会尝试匹配未绑定对象。匹配成功后，driver 的 `probe` 初始化具体硬件或传输层，成功后设备记录对应 driver，driver 也记录自己已经管理的设备。

`DeviceBase` 中的 `drv_state` 是这里的关键点。设备模型只负责保存一个类型擦除的私有状态槽，不解释状态内容；具体 driver 在 probe 成功后写入自己的状态对象，后续 shutdown、interrupt handler 或设备文件操作再按真实类型取回。这样，通用设备框架不需要知道 VirtIO block、串口、PCIe transport 等具体实现的数据结构，驱动私有状态也不会反向污染 VFS 或 syscall 层。

通过过这种机制，我们就漂亮地解耦了VFS和设备驱动系统。

=== Bus

Anemone 目前已经支持三类总线：Platform、VirtIO 和 PCIe。它们共用 `BusType` 的设备 / 驱动集合与 probe 流程，但匹配规则不同。

Platform bus 面向设备树发现出来的 MMIO 设备。`PlatformDevice` 持有固件节点和 MMIO resource，`PlatformDriver` 暴露一组 compatible 字符串，总线通过设备树中的 `compatible` 属性选择 driver。这让串口、VirtIO MMIO transport、RTC、platform interrupt controller 等设备可以通过同一类固件描述进入设备模型。

VirtIO bus 位于 transport 之后。无论底层 transport 来自 MMIO 还是 PCIe，transport driver probe 成功后都会创建一个 `VirtIODevice`，再把它注册到 VirtIO bus。VirtIO driver 不关心 transport 的来源，只根据 VirtIO device type 匹配。例如 VirtIO block driver 只需要声明自己支持 block device type，实际设备可以来自 QEMU virt 的 MMIO，也可以来自 PCIe 枚举。

PCIe bus 的匹配和初始化更复杂。它既可以按 vendor / device id 匹配，也可以按 class code 匹配；probe 之前还要完成 BAR 探测、aperture 分配和 MMIO remap。Anemone 把这些资源分配放在 PCIe bus / PCIe driver 接口内完成，所有复杂度都不泄露出去。

=== Device

`Device` 是设备模型中的拓扑节点。设备对象可以有父子关系，例如 platform 设备下面生成 VirtIO MMIO transport，transport 再生成具体 `VirtIODevice`；PCIe host bridge 枚举出 endpoint，VirtIO PCIe transport 又把 endpoint 转换成 VirtIO 设备。这个拓扑让 shutdown 可以按深度优先顺序通知子设备先退出，再处理父设备，避免父 transport 先消失后子设备还尝试访问硬件。我们在关机之前，就会先沿着设备树拓扑逐个关闭设备，正确的顺序确保了我们不会出现“先关闭父设备，再关闭连着父设备的子设备”的错误行为。

设备对象还连接固件描述和 driver 私有状态。Platform device 从设备树节点读取 compatible、reg、interrupt-parent 等属性；PCIe device 保存配置空间、BAR 和 domain 资源；VirtIO device 保存 transport。通用 `Device` trait 只暴露少量能力：取回当前绑定的 driver、记录 driver state、增加子设备和访问固件节点。这样的接口足够支撑通用 probe 流程，但不会让下层驱动依赖不属于自己的内部状态。

=== Driver

`Driver` 的核心入口是 `probe`。probe 成功意味着 driver 已经接管该设备，并且可以把后续需要的状态写入 `drv_state`。

不同总线通过 `as_platform_driver`、`as_virtio_driver`、`as_pcie_driver` 取得自己的窄接口。Platform driver 暴露 match table，VirtIO driver 暴露 device id table，PCIe driver 暴露 class code / vendor-device table 和资源初始化 hook。这种区分让通用 `DriverOps` 不需要知道每种总线的匹配细节，总线也不需要猜测某个 driver 的私有类型。

== 字符设备与块设备

设备被 driver 接管后，还需要以用户态可访问的形式暴露出来。Anemone 把这一步放在字符设备和块设备子系统中完成。字符设备提供字节流语义，块设备提供固定块读写语义；二者都先注册到自己的 subsystem，再由 subsystem 发布到 `devfs`。

#code-block(
  ```rust
  pub trait CharDev: Send + Sync {
      fn devnum(&self) -> CharDevNum;
      fn read(&self, buf: &mut [u8]) -> Result<usize, SysError>;
      fn write(&self, buf: &[u8]) -> Result<usize, SysError>;
      fn seek(&self, ctx: CharSeekCtx<'_>) -> Result<usize, SysError>;
      fn ioctl(&self, ctx: CharIoctlCtx<'_>) -> Result<u64, SysError>;
  }

  pub trait BlockDev: Send + Sync {
      fn devnum(&self) -> BlockDevNum;
      fn block_size(&self) -> BlockSize;
      fn total_blocks(&self) -> usize;
      fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError>;
      fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError>;
      fn ioctl(&self, ctx: BlockIoctlCtx<'_>) -> Result<u64, SysError>;
  }
  ```.text,
  caption: [`CharDev` 和 `BlockDev` 分别定义字符设备与块设备的最小能力接口],
  lang: "rust",
)

字符设备侧，比如`/dev/null`、`/dev/zero`、`/dev/full`、`/dev/urandom` 和串口都通过同一类 `CharDev` 接口暴露读写语义。统一的 char devfs file ops 只负责从 inode 的 `rdev` 找到对应设备，再把 read、write、seek 和 ioctl 分发给 `CharDev`。这样，内存类字符设备的 seek 行为、串口的读写行为、随机数设备的读取行为都留在具体设备 owner 内部，而不是写成 devfs 的设备号特判。

块设备侧，后端 `BlockDev` 只承诺按 block size 对齐的块读写。用户态通过 `/dev/vda`、`/dev/loop0` 等块特殊文件访问时，devfs 的 block file ops 在前端提供封装后的、字节级读写（这一点也是为了对齐Linux）和seek：对非对齐读写使用中间缓冲做 read-modify-write，对 `BLKGETSIZE64`、`BLKSSZGET` 等通用 `BLK*` ioctl 先在 block subsystem 内处理，剩余私有命令再交给具体 `BlockDev::ioctl`。这使后端驱动可以保持清晰的块设备契约，同时让 Linux 用户态看到接近普通块特殊文件的字节接口。

== 设备树与平台初始化

Anemone 的平台发现以设备树为主要入口。启动早期，内核从固件传入的 FDT 中扫描内存、CPU 数量和时钟频率等必须在完整初始化前知道的信息；进入通用初始化后，内核把 FDT unflatten 成长期驻留的设备树对象，并通过路径、phandle、compatible、reg、ranges 等接口服务后续平台发现。

我们实现了自己的 `device-tree` crate 来保存 unflatten 之后的树结构，它已经通过了Miri的验证。`DeviceTreeHandle` 持有 arena-backed 的不可变树，`DeviceNodeHandle` 可以稳定引用其中节点；平台发现逻辑据此遍历 `simple-bus` 下的子节点，为带 compatible 且 status 可用的节点创建 `PlatformDevice`，解析 MMIO resource，并注册到 Platform bus。进入这一步之后，设备发现就不再依赖架构启动代码里的硬编码分支，而是回到统一的 bus matching 和 driver probe。

在设备树之外，Anemone 还保留了一层很薄的 `MachineDesc`。它用来根中断控制器、早期 timer 这类必须在完整驱动设备初始化前完成的初始化。

#code-block(
  ```rust
  pub trait MachineDesc: Sync {
      fn compatible(&self) -> &[&str];
      unsafe fn early_init_intc(&self);
      unsafe fn early_init_timer(&self);
  }
  ```.text,
  caption: [`MachineDesc` 只覆盖完整设备驱动模型建立前必须完成的板级早期入口],
  lang: "rust",
)

这个设计参考了 Linux ARM 的 `machine_desc` / `DT_MACHINE_START` 思路。后来的通用架构越来越多把平台初始化推向设备树驱动的统一发现路径，但我们没有完全走纯设备树路线：Anemone 当前需要支持 RISC-V 和 LoongArch 两套启动入口，根中断控制器和早期 timer 又必须先于普通 platform probe 建立。我们把这些少量板级差异压缩到 `MachineDesc` 中，再把常规设备交给设备树和 Platform bus。这样既保留了设备树的可扩展性，也避免为了少数早期入口把通用设备模型设计得过重。

相比不少往届作品中常见的板级硬编码初始化，这套抽象让迁移新平台的改动更集中。新的平台通常需要补充机器描述、设备树节点和少量 driver match table，而不是在主初始化流程里散布新的条件分支。对 Anemone 来说，这正好满足当前阶段的需求：复杂度可控，同时具备向更多 QEMU machine 或真实开发板迁移的空间。

== Devfs 桥接

`devfs` 是设备模型与 VFS 之间的桥。设备子系统完成注册后，可以可选地把设备发布到设备文件系统；`devfs` 保存稳定的名字、inode 编号、权限和 `rdev`，但不拥有具体设备语义。打开设备节点时，`devfs` 调用发布记录中的 `DevfsNodeOps::open()`，由字符或块设备子系统返回真正的文件操作表。

#code-block(
  ```rust
  pub trait DevfsNodeOps: Send + Sync {
      fn open(&self, inode: &InodeRef) -> Result<OpenedFile, SysError>;
      fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr)
          -> Result<InodeStat, SysError>;
  }
  ```.text,
  caption: [`DevfsNodeOps` 只负责把 devfs inode 接回设备子系统拥有的 file behavior],
  lang: "rust",
)

简单来说——driver probe 产生设备后端，char / block subsystem 注册设备号和名字，devfs 发布 inode，VFS open 得到设备子系统提供的 `FileOps`。之后 read、write、seek、ioctl 都沿着 `FileOps` 回到对应的 `CharDev` 或 `BlockDev`。因此，`/dev` 看起来是文件系统命名空间的一部分，但设备行为仍然由设备 owner 决定。

这种设计让文件系统章节和设备驱动章节有清楚分工。VFS 负责路径、挂载、inode 和 opened file description；设备驱动模型负责硬件发现、驱动绑定和设备能力；`devfs` 只承担发布和分发，不把 VFS 变成驱动框架，也不把驱动框架变成路径查找系统。对于后续扩展 sysfs、更多字符设备、更多块设备或热插拔协议，这个边界也为我们保留了继续演进的空间。
