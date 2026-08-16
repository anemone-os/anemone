#import "../components/figure.typ": code-block, report-figure

= 内存管理

Anemone的内存管理主要可以分为物理页管理、页表映射、用户地址空间几个相对清楚的部分。

== 物理页管理

内核启动早期会根据固件提供的内存描述注册可用物理内存区域。物理页分配器接收这些区域后，用 buddy 算法管理连续页分配；上层可以申请单页，也可以申请一段连续页。分配器本身不保存复杂的对象语义，只维护页是否可分配和当前页数统计。

*Anemone实现了单独的buddy system库，并经过了Miri和cargo-fuzz的检验。*相比于往届内核常用的buddy-system-allocator crate，Anemone的buddy system库不再反过来依赖alloc crate，而是采用侵入式的方式，把页分配器的状态直接嵌入到内核的静态内存中。这样可以避免在内核早期初始化阶段使用alloc crate带来的复杂依赖和潜在问题。实际上，促使我们开发这个库的一大原因正是：一开始使用buddy-system-allocator crate时，堆内存耗尽时，向页分配器申请页，但页分配器内部又依赖堆，这直接导致了一个“死锁”，实际上是死循环。

页的生命周期由 RAII 句柄约束。`FrameHandle` 表示对一个物理页的引用，克隆句柄会增加引用计数；句柄释放时减少引用计数，最后一个引用消失后再把页还给 buddy 分配器。`OwnedFrameHandle` 表示独占页，只有在页没有共享引用时才能取得；需要写入页内容的路径优先使用独占句柄。

#code-block(
  ```rust
  #[derive(Debug)]
  pub struct FrameHandle {
      ppn: PhysPageNum,
  }

  #[derive(Debug)]
  pub struct OwnedFrameHandle {
      inner: FrameHandle,
  }
  ```.text,
  caption: [两种类型内部相同，差别在于暴露的语义],
  lang: "rust",
)

Anemone参考Linux的memmap设计，创新性地引入了基于基数树的memmap管理。每个物理页可以直接通过其物理页号在O(1)时间内找到对应的memmap结构体。memmap结构体中包含了该物理页的状态信息，比如是否被分配，是否被锁定等。同时，memmap结构体还包含了指向该物理页所属的内存区域的指针，这使得我们可以快速地找到该物理页所在的内存区域，从而进行更高效的内存管理操作。

#code-block(
  ```rust
  #[derive(Debug)]
  pub struct Frame {
      ppn: PhysPageNum,
      // Reference count
      rc: AtomicUsize,
  }
  ```.text,
  caption: [物理页的元数据],
  lang: "rust",
)

== 内核堆

调研往届内核时我们发现，大部分作品的堆都是固定大小的，一开始就被嵌入内核的bss段中。这导致了内核堆的大小无法动态调整，限制了内核的灵活性。一旦发现堆不够大，就必须重新编译内核，或者在运行时进行复杂的内存管理操作。

Anemone的内核堆是动态可扩展的，它可以根据需要在运行时进行扩展。当堆内存不够时，它会向底层页分配器请求更多的物理页，并将这些页映射到内核堆的虚拟地址空间中。这样，内核堆的大小可以根据实际需求进行调整，提高了内核的灵活性和可扩展性。

我们使用 Talc 作为全局分配器。启动早期先使用一段 bootstrap heap；当堆空间不足时，堆的 OOM handler 会向物理页分配器申请新的页，把这些页加入 Talc 的可分配 span。这样内核堆不需要在链接脚本中固定一个很大的静态上限，实际使用量由运行时需求推动。

== 分页

`PageTable` 持有硬件页表根页，具体的遍历、映射、解除映射和权限修改由 `Mapper` 完成。这个边界让架构相关代码只需要提供页表项格式、地址空间切换、TLB 刷新等硬件能力，通用内存管理逻辑则复用同一套映射计算。

页表页本身也来自物理页分配器。创建地址空间时，内核先分配并清零根页，再复制内核共享映射；销毁地址空间时，`PageTable` 会解除用户态范围内的映射，并把页表页按受管页重新交还。这样，我们就通过RAII确保了页表页的生命周期和地址空间对象的生命周期一致，避免了悬空页表页或内存泄漏的问题。

#code-block(
  ```rust
    /// PageTable. The container of page directories.
    ///
    /// The mapping/unmapping logic is implemented by mappers.
    #[derive(Debug)]
    pub struct PageTable {
        root: PhysPageNum,
    }

    /// Mapper. Computation engine for page table traversal and modification.
    #[derive(Debug)]
    pub struct Mapper<'a> {
        root: PhysPageNum,
        _lifetime: PhantomData<&'a mut PageTable>,
    }
  ```.text,
  caption: [`PageTable` 持有页表根页，`Mapper` 提供映射操作],
  lang: "rust",
)

== 用户地址空间

Anemone中，我们使用`UserSpace`表示一个用户进程的地址空间，内部包含页表、按起始虚拟页组织的 VMA 集合、用户栈和堆的保留区，以及 SysV shared memory 的 attachment 记录，等等。新建地址空间时，内核会建立零页 guard、栈保留区和 heap 保留区；真正的物理页通常延迟到第一次访问时再分配，这样可以避免为未使用的内存浪费物理页。

#code-block(
  ```rust
    #[derive(Debug)]
    pub struct UserSpace {
      /// Underlying hardware page table.
      table: PageTable,

      /// User virtual memory areas, including stack and heap.

      vmas: BTreeMap<VirtPageNum, VmArea>,
      /// SysV shared memory attachments by page-aligned attach start.

      sysv_shm: BTreeMap<VirtPageNum, shm::ShmAttachment>,

      stack: Stack,

      heap: Heap,

      /// Command-line argument region. [start, start + size). Strings, not
      /// pointers.
      ///
      /// /proc/[id]/cmdline needs this.
      cmdline_range: Final<(VirtAddr, usize)>,
      /// Environment variable region. [start, start + size). Strings, not
      /// pointers.
      ///
      /// /proc/[id]/environ needs this.
      env_range: Final<(VirtAddr, usize)>,

      ...
  }
  ```.text,
  caption: [用户地址空间对象],
  lang: "rust",
)

#pagebreak(weak: true)
== ELF 按需装载

初赛阶段的 ELF 装载会在 `execve` 时把程序段较早地读入内存。进入决赛后，面对 glibc、Rust 工具链和大量动态库，这种方式会带来明显的启动开销和内存浪费：一个程序映像中有相当一部分代码和数据在本次运行里根本不会被访问。

我们把 ELF load segment 改造成了按需分页的 VMO backing。`execve` 仍然负责检查 ELF 结构、建立地址空间和装载主程序及解释器，但段内容不再全部立即复制；真正访问某一页时，缺页路径才从对应文件页缓存取得内容，并按 ELF 中 file size 与 memory size 的关系补零。只读、页对齐的内容可以直接复用文件页，私有可写段则继续通过 ShadowObject 获得写时复制语义。

这项改造把程序装载自然地接入了已有 VMO 架构，而不是另建一套 ELF 专用缓存。它直接降低了大型程序启动时的磁盘读取和物理页占用，也使动态链接器、编译器和大量短生命周期子进程更适合在 Anemone 上运行。

== VMO 与写时复制

我们认为这是Anemone最突出的特征之一：*Anemone没有照搬Linux，而是参考现代微内核（Fuchsia/Zircon）的设计理念，引入了VMO（Virtual Memory Object，虚拟内存对象）架构。*

#code-block(
  ```rust
  pub trait VmObject: Send + Sync {
      /// Resolve the frame at `pidx` for the given access type.
      fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError>;

      ...
  }


  ```.text,
  caption: [VmObject trait 定义了虚拟内存对象的核心接口],
  lang: "rust",
)

VMO 是用户内存的后端对象。它的核心接口是按对象内页号和访问类型解析一个物理页，并告诉 VMA 这页是否允许以可写权限映射。VMA 不需要知道页来自匿名内存、文件页缓存、共享内存还是 ELF 装载段；它只根据自身权限和 VMO 返回结果安装页表项。

匿名 VMO 在读 fault 上可以返回共享零页，避免为从未写过的匿名页立即分配物理内存；写 fault 才分配并清零一个真实物理页。固定页 VMO 用于 ELF 段等已经准备好的页集合。文件系统 inode 可以暴露自己的 mapping，文件 mmap 因此和普通文件页缓存使用同一类 backing。SysV shared memory 的 VMO 则在第一次访问时物化真实页，即使第一次访问是读，也必须分配共享页，因为后续其他 attach 者的写入需要通过同一帧可见。这些形形色色的复杂语义被抽象到统一的VMO之后，而前端的VMA完全不需要关心这些细节！

#report-figure(
  image("../assets/vmo.png", width: 80%),
  caption: [形形色色的VMO被抽象在统一的VmObject trait之后],
)

VMO架构的关键就在于`ShadowObject`。它承担私有映射和 fork 后写时复制的核心语义。读和执行访问会沿父对象解析页面，并把返回页降级为不可写；写访问则复制父对象当前页内容，把新页记录在 shadow overlay 中。之后同一页的访问优先命中 overlay，不再影响父对象或兄弟地址空间。通过这个设计，*Anemone自然地实现了COW、匿名页懒分配与全局只读零页共享，*从而大大减少了内核的复杂性和潜在的错误，提高了内存利用率。

#report-figure(
  image("../assets/vmo-fork.png", width: 100%),
  caption: [一个VMO经过fork，会产生三个新的VMO],
)

== TLB 与多核地址空间

页表修改之后，处理器中已经缓存的地址转换也必须同步更新。我们最初采用非常保守的策略：一旦用户页表发生变化，就刷新本地 TLB，并通知所有其它 CPU。它容易保证正确，却会让缺页、`mmap`、`fork` 和进程退出在多核环境中付出较高代价。

决赛阶段，我们让页表操作区分“新增映射”和“替换或收紧已有映射”。新的映射不存在旧转换，普通用户缺页返回时可以利用硬件重新执行指令完成自然装载；真正可能残留旧转换的修改仍然执行必要的失效。对于远端 CPU，地址空间会记录哪些处理器仍可能持有自己的转换，只向这些 CPU 发送 shootdown，而不是无条件广播到所有核心。

我们还为连续地址范围提供批量失效，避免逐页发送重复指令或 IPI。经过这些改进，TLB 维护从“每次都做最保守的全局动作”变成了与修改性质和地址空间活动范围相匹配的操作，既保持双架构上的正确性，也显著减少了大型编译负载中的额外开销。

== OOM Killer

Anemone对于OOM具备一定的防御能力。在内核启动时，我们会创建 OOM Killer 内核线程，并由周期采样检查系统内存水位。一旦可用内存低于可配置阈值，OOM Killer 会寻找占用独占物理页较多的用户进程并发送 `SIGKILL`，使进程沿正常退出路径释放地址空间和文件资源。

OOM Killer 将在内存水位恢复安全后继续睡眠，平时不会占用 CPU 空转。配合匿名页懒分配、ELF 按需装载、写时复制以及 `brk` 收缩时的页面回收，这套机制让 Anemone 能够更从容地承载编译器等内存需求波动较大的真实程序。
