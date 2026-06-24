#import "../template/components.typ": *
#import "../template/figures.typ": *

= 内存管理与 memory object

#epigraph(attribution: [Alan Kay])[
  Simple things should be simple, complex things should be possible.
]

#thesis[
  Anemone 的内存管理不是 Linux VM 的缩写版，也不是 Zircon VMO 的完整移植。它走的是一条折中路线：用户态看到的是 Linux-visible `mmap`、`brk`、`mremap`、`msync` 和 SysV shm 语义，内核内部则用 address space、VMA、page table 和 backing object 把“虚拟地址可见性”和“页面从哪里来”分开。这样做让 Anemone 能逐步补齐 Linux 兼容行为，同时不让每个 ABI 角落反向污染内存对象的 owner boundary。
]

在很多内核里，“内存管理”会自然膨胀成一个大而全的主题：物理页分配、地址空间、page fault、ELF 装载、匿名页、文件映射、page cache、共享内存、OOM、锁页、procfs 统计都能放进来。Anemone 把这条路径收束在 address space、VMA、page table 与 backing object 的分工上。物理页分配提供页框，page table 提供硬件映射，`UserSpace` 维护进程地址空间的 VMA registry、heap、stack 和 SysV shm attachment；真正把这些层接起来的是 fault path 上的 backing object。

这里的 backing object 在代码里叫 `VmObject`。这个名字很接近 Fuchsia / Zircon 的 VMO 语境，但 Anemone 只采用了其中一部分思想：把“映射看到哪一段对象”和“对象如何解析页面”拆成可审查的接口。它还不是完整的 capability-oriented VMO system，不承担全局 handle policy、rights model、pager protocol 或完整的用户可见对象 ABI。这个负空间很重要，因为它解释了为什么 Anemone 可以吸收 memory object 的边界思想，却不需要把内部结构伪装成另一个内核的对象模型。

#boundary[
  在 Anemone 语境中，memory object / backing object 指内部页面来源抽象，主要对应 `VmObject::resolve_frame()`、`sync_range()`、`discard_range()` 和少量驻留页统计接口。它不是公开 UAPI，也不是完整 Zircon-style VMO 承诺。
]

== Address space 与 backing object

`UserSpace` 是用户地址空间的 owner。它持有硬件 page table、按起始 VPN 索引的 `VmArea` 集合、stack / heap reservation、命令行和环境变量范围，以及 SysV shm attachment 表。它不把每一页都提前装进 page table；相反，VMA 描述“这个虚拟区间允许什么访问、fork 时如何处理、从 backing object 的哪个 page index 开始看”。

这个分层让 `brk` 的语义很直接。`sys_brk` 只调整 heap reservation 中的 program break；缩小时拆掉已经超过新 break 的页表映射并做 TLB shootdown，增长时通常不立即分配物理页。真正的页框分配留给后续缺页处理。于是 `brk` 是 Linux-visible heap control surface，`Heap` / heap VMA 是 address-space 内部状态，而 `AnonObject` 才是未来页面的来源。

#listing([`VmArea` 把虚拟区间、object 偏移、权限和 backing object 绑定在一起])[
  ```rust
  pub struct VmArea {
      range: VirtPageRange,
      poffset: usize,
      prot: Protection,
      on_fork: ForkPolicy,
      flags: VmFlags,
      reservation: Option<VmReservation>,
      backing: Arc<dyn VmObject>,
  }

  pub trait VmObject: Send + Sync {
      fn resolve_frame(
          &self,
          pidx: usize,
          access: PageFaultType,
      ) -> Result<ResolvedFrame, SysError>;
  }
  ```
]

这个接口形状比“VMA 里直接存一堆物理页”更有用。匿名映射、ELF segment、file-backed mapping 和 SysV shm 都可以变成 `VmArea -> VmObject` 的组合；同一个 VMA 编辑器负责 map、unmap、protect、discard、remap 的区间操作，而不同 backing object 负责自己的页面解析和驻留状态。

== Page fault owner boundary

用户态 page fault 从 arch / exception 层进入 `handle_user_page_fault()`。顶层只构造 fault info，拿到当前 task 的 `UserSpaceHandle`，再进入 `UserSpace::handle_page_fault()`。如果地址不在可访问 stack、heap 或已有 VMA 里，或者 backing object 不能提供页面，当前顶层把错误收束成同步 `SIGSEGV` 投递给 faulting task。更细的 file-backed EOF / hole 页和 SysV shm 只读写 fault 语义，则属于各 backing owner 的错误域，而不是 trap 顶层的统一判断。

在正常路径上，`UserSpace` 找到包含 fault address 的 `VmArea`，检查 fault 类型是否被 VMA protection 允许，把 VPN 转成 object-relative page index，然后调用 backing object 的 `resolve_frame()`。返回的 `ResolvedFrame` 决定实际映射的物理页，以及即使 VMA 允许写时该页是否真的能 writable。最后 page table 安装 PTE，并执行本地 TLB shootdown。

#book-figure(
  "../assets/figures/ch07/page-fault-owner-boundary.png",
  [Page Fault 连接 trap handling、address space 和物理页分配。],
  width: 100%,
)

这个路径的收益是 owner boundary 清楚：trap 层不理解匿名页、文件页或 SysV shm；VMA 不直接分配所有物理页；backing object 不持有整个 task；frame allocator 也不决定用户地址空间策略。它们通过 fault info、object page index、resolved frame 这几个窄接口相接。

== 匿名页、COW 与 file-backed mapping

匿名映射由 `AnonObject` 提供。读 fault 或执行 fault 可以先返回全局共享零页，写 fault 才分配真实 zeroed frame 并写入 object 的 resident page map。fork 时，`ForkPolicy::CopyOnWrite` 会把 parent 和 child 都切到 `ShadowObject`，写入时复制 parent page 到 overlay。这个模型不追求一次性模拟 Linux 的全部 anon-vma 细节，但它把 COW 的 owner 明确放在 backing object 链上，而不是散落在 fault 顶层。

file-backed mapping 则从 inode 拿 `mapping()`。shared mapping 直接使用 inode 的 mapping；private mapping 用 `ShadowObject` 包一层，使写入落到私有 overlay。ext4 和 ramfs 都把 regular file state 实现为 `VmObject`：ramfs 在洞页读取时可以返回零填充并按文件大小限制 mmap 范围；ext4 维护 resident page map、dirty bit 和 backing file cache page counter，`msync` 通过 `sync_range()` 把脏页写回。truncate 会更新 inode size 并裁剪 resident cache，但当前不会主动失效已经安装到用户 page table 的 live mmap PTE。

这就是 Anemone file-backed mapping 的真实边界。page cache 作为文件 backing 与 fault 之间的共享层，让 ordinary read/write 与 mmap fault 通过同一份 regular-file mapping 交汇；但 Linux 级 truncate/mmap 强一致性、文件洞页、EOF 后 `SIGBUS`、VMA guard hole 和 readonly view 下的 shared writable mmap 都是更细的可观察语义，需要由 filesystem、VFS view、fault owner 和 page-cache owner 分别给出规则，不能靠 VMA 形状自动得到。

#tradeoff[
  把 file-backed page cache 做成 inode mapping 的 backing object，可以让 VFS read/write、mmap fault、`msync` 和 inode shrinker 看到同一批 resident pages。代价是 Linux 的 mmap coherency 细节必须逐步补齐：truncate shrink、hole zero-fill、EOF `SIGBUS`、shared writable mmap 和 dirty/writeback 不能靠 VMA 形状自动得到。
]

#book-figure(
  "../assets/figures/ch07/backing-object-map.png",
  [mapping 通过 backing object 连接匿名页、文件页和共享内存。],
  width: 100%,
)

== SysV shm backing、权限与生命周期

SysV shm 是共享内存模型中最能暴露边界压力的路径，因为它同时触碰 Linux ABI、共享 backing、权限检查和生命周期。`shmget` 创建或查找 segment，registry 用 slot index、sequence、key map 和 page quota 管理 Linux-visible id；segment 记录 size、creator、permission metadata、attach count、remove bit 和 last attach/detach/change time；真正的页面内容放在 `ShmObject` 里。

`ShmObject` 与普通 anonymous object 的关键差异是共享读也要 materialize 真实 zeroed page。普通匿名页首次读可以用 shared zero frame，因为写入时会 COW 或分配私有页；SysV shm 的两个 attachment 必须看到同一帧，所以第一次读 fault 就需要把页面放进 segment 的 resident map。这个选择让共享语义的 owner 回到 shm segment，而不是让每个 attach 的 VMA 自己维护一份“看起来共享”的状态。

`shmat` 路径先在 registry 锁内 reserve attach，提高 attach count，避免并发 `IPC_RMID` 在 VMA 安装前回收 slot；随后按 `SHM_RDONLY`、默认 read-write 和 `SHM_EXEC` 计算 VMA protection，并用 shm-local credentials view 做 SysV IPC DAC check。真正安装时，`UserSpace::attach_sysv_shm()` 通过 `ObjectMapping` 把 segment 的 `ShmObject` 映射进地址空间，并登记 attachment。`shmdt`、fork、exec 和进程退出再驱动 detach 和 attach count 维护。

这里也有明确边界。SysV shm 的 attach 生命周期由 `shmdt`、fork、exec 和进程退出维护；用户直接 `munmap` shm 区间如果要等价于 SysV detach，就必须由地址空间和 shm registry 建立更明确的反向通知协议。`SHM_LOCK` / `SHM_UNLOCK` 可以有权限 gate 和 Linux-visible mode bit，但真正的 page pin、resident-page accounting 和 `RLIMIT_MEMLOCK` 需要内存管理 owner 提供完整承诺。

#boundary[
  SysV shm 不是“匿名 shared mmap 的别名”。它有独立 registry、id/sequence、permission metadata、remove-after-last-detach 生命周期和 Linux-visible `shmctl` 统计。共享页面内容由 `ShmObject` 拥有；地址空间只拥有 attachment 视图。
]

== 内存压力路径

OOM killer 和 inode shrinker 也会接触内存对象，但它们只是内存压力路径的观察者和回收者，不是 VM 事实源。`UserSpace::exclusive_physical_pages_snapshot()` 只统计释放该地址空间时可能立即归还给 frame allocator 的独占物理页，跳过 shared VMA，并递归处理部分 exclusive shadow parent。这个 snapshot 只服务 OOM victim ordering；它不是 `/proc` RSS，也不是 resident accounting 的长期 ABI。

inode shrinker 则从 VFS resident inode cache 和 backing file page cache 侧降低压力。它只覆盖 filesystem resident inode / backing-file cache 的 opportunistic 回收，不处理 anonymous pages、SysV shm、slab 或完整 generic shrinker policy。这个边界与本章主线一致：内存压力机制可以观察 backing object 和 cache，但不能把自己的临时 score 写成用户可见 VM 语义。

== TradeOff: Backing object 边界与 coherency 成本

Anemone 通过 address space / VMA / backing object 三层，把 Linux-visible 内存接口压到 syscall adapter 和少数 typed mapping 入口，把页面来源压到 `VmObject` owner，把硬件映射压到 page table/fault path。`mmap` 不需要知道 ext4 如何读页；ext4 mapping 不需要知道 task topology；SysV shm 不需要伪装成普通文件；OOM killer 也不需要制造一套 RSS 真相源。

这个设计的代价是 Linux 可观察接口不会自动随 `VmArea` 出现。file-backed fault 的错误域、truncate 与 live mmap coherency、`mremap` 的 backing-aware grow/move、heap `mprotect` 的持久化、锁页账本、`/proc/self/maps` / pagemap / `/dev/zero` / rlimit，都需要对应 owner 给出各自的 contract。Anemone 当前已经建立的是 address space、fault path 和 backing object 的结构边界，而不是完整 Linux VM 复刻。
