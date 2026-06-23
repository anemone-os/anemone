#import "../template/components.typ": *
#import "../template/figures.typ": *

= ABI 边界与系统调用层

#thesis[
  Anemone 追求的是 Linux-visible syscall surface 的兼容，同时保留 Anemone-native syscall/UAPI surface 作为受控扩展面。系统调用层的职责，是把 syscall number、raw register、Linux UAPI flag、native UAPI、errno、用户指针和兼容性噪声收束在边界附近；越过这层之后，内部对象应当看到 typed intent、owner-local context 和 Anemone 自己的状态边界。
]

系统调用层很容易被写成“巨大的 switch + 一堆 wrapper”。这当然能跑，但它会把 Linux ABI 的细节扩散到每个子系统：某个 flag 为什么被静默接受、某个 fd 为什么是 `EBADF` 而不是 `EINVAL`、某个用户指针什么时候该变成 `EFAULT`，都会变成内部对象不得不理解的背景知识。Anemone 的做法更接近一层 adapter：外部保持 Linux 形状，内部尽量回到自己的对象模型。

#book-figure(
  "../assets/figures/ch02/syscall-adapter.png",
  [系统调用层把 Linux-visible surface 与 Anemone-native UAPI 收束为内部 owner 可以理解的 typed intent。],
  width: 100%,
)

== 系统调用注册表把分散定义收束为集中分发和边界样板

Anemone 的 syscall handler 并不集中手写在一个大表里。子系统在自己的 API 文件附近定义 handler，`#[syscall(...)]` 过程宏负责生成 wrapper 和 handler metadata，再把 metadata 放入 `.syscall` 链接段。启动时，`register_syscall_handlers()` 读取链接器提供的 `__ssyscall` / `__esyscall` 范围，检查对齐、大小、syscall number 范围和重复注册，然后填入唯一的 `SyscallTable`。

#listing([`#[syscall]` 同时描述 raw preparse、validator、typed conversion 和注册 metadata])[
```rust
fn trace_execveat(
    dirfd: u64,
    path: u64,
    argv: u64,
    envp: u64,
    flags: u64,
) {
    kdebugln!("execveat raw: path={path:#x} flags={flags:#x}");
}

fn nullable_string_array<const N: usize, const M: usize>(
    raw: u64,
) -> Result<Vec<Box<str>>, SysError> {
    if raw == 0 {
        Ok(Vec::new())
    } else {
        c_readonly_string_array::<N, M>(raw)
    }
}

impl TryFromSyscallArg for ExecveAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let bits = syscall_arg_flag32(raw)?;
        Self::from_bits(bits).ok_or(SysError::InvalidArgument)
    }
}

#[syscall(SYS_EXECVEAT, preparse = trace_execveat)]
fn execveat(
    dirfd: i32,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    #[validate_with(nullable_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)]
    argv: Vec<Box<str>>,
    #[validate_with(nullable_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)]
    envp: Vec<Box<str>>,
    flags: ExecveAtFlags,
) -> Result<u64, SysError>

#[used]
#[unsafe(link_section = ".syscall")]
static __SYSCALL_EXECVEAT: SyscallHandler = ...
```
]

syscall 定义因此可以靠近真正拥有语义的子系统：`openat` 留在 VFS API 附近，`execve` 留在 task loading 边界附近，`ioctl` 留在文件 API 附近，而不是被迫回到一张中心清单。分发表仍然是启动时构造的一份全局真相源；重复 number、越界 number 和链接段形状错误会在注册阶段暴露，而不是延迟到稀有 syscall 路径上才变成运行时故障。

宏生成的 wrapper 也不是单纯的调用胶水。`preparse` 接收带名字的 raw `u64` 寄存器参数，适合在任何转换前做日志或保留诊断上下文；带 `#[validate_with(...)]` 的参数走显式 validator，例如 C 字符串、字符串数组和用户地址；其余参数默认通过目标类型的 `TryFromSyscallArg` 转换，例如 fd wrapper、flag wrapper 或 signal number。这样，边界层既能保留 Linux/raw 输入的可观测性，又能把内部函数签名压成 typed 参数。

#rationale[
  link-section registry 和 syscall macro 解决的是“定义分散、分发集中、参数解析一致”的张力：语义靠近 owner，调度入口仍保持一张可检查的 syscall table，raw argument 也不会绕过统一的解析边界。
]

#book-figure(
  "../assets/figures/ch02/syscall-registry.png",
  [链接段注册表让 syscall 定义留在 owner 附近，同时在启动阶段形成一张集中、可检查的分发表。],
  width: 100%,
)

== Native syscall 是受控扩展面，不是兼容捷径

Anemone 也保留自己的 native syscall 区间。`anemone-abi` 中，Linux syscall number 空间之后的 `SYS_ANEMONE_START` 用来放置 Anemone-specific syscall；当前已经有 `SYS_DBG_PRINT` 和 `SYS_POWER_SHUTDOWN` 这类只对 Anemone runtime 有意义的控制面。它们不是 Linux compatibility surface 的一部分，也不应伪装成某个 Linux syscall 的替代实现。

这个 native surface 仍然穿过同一套 syscall registry、参数解析和用户指针边界。`SYS_DBG_PRINT` 使用 `#[validate_with(c_readonly_string<...>)]` 从用户态读取受限长度字符串；`SYS_POWER_SHUTDOWN` 则用 magic value 约束关机控制。区别在于语义来源：Linux syscall 的外部契约来自 Linux ABI，native syscall 的外部契约来自 Anemone 自己愿意稳定暴露的 UAPI。

#boundary[
  native syscall 不能成为绕过 Linux 兼容缺口的捷径。面向通用用户态程序的行为仍应优先落在 Linux-visible surface；native UAPI 只表达 Anemone 自身需要稳定暴露的控制面、运行时能力或调试能力。
]

== 参数解析先于内部语义

硬件 trap 进入内核后，`handle_syscall()` 从 `TrapFrame` 取出 syscall number 和六个 raw argument，提前推进 syscall PC，再调用选中的 handler。handler 返回 `SysError` 时，边界层把它映射成 Linux 风格的负 errno 返回值；如果错误本身携带 `RestartSyscall`，`handle_syscall()` 会把它提取成额外的 `Option<RestartSyscall>`，供 trap-return 和等待路径继续处理。

raw argument 不直接进入内部对象。`#[syscall]` 生成的 wrapper 会逐个调用 `parse_syscall_arg()`：普通参数走 `TryFromSyscallArg`，特殊参数可以用 `#[validate_with(...)]` 指定边界验证函数。解析失败会带上 task、sysno、handler、argument index 和 raw value 打日志。这不是为了把日志写漂亮，而是为了让 ABI 边界上的错误仍然可定位。

用户指针也遵守同一个原则。`user_access` 先检查地址是否落在 user-space 上界之内，再按读写方向验证映射和权限，并把一部分地址空间错误归一到 `BadAddress`。越过这层之后，子系统拿到的是 `UserReadPtr`、`UserWritePtr`、用户空间 handle 或已经 copy 出来的内核对象，而不是随手解引用的整数地址。

#boundary[
  syscall 参数是用户态输入，不是内部 API。内部对象不应依赖 raw register 的编码细节；如果某个 Linux flag、mode 或指针布局必须被理解，它应当尽量在 syscall adapter 附近被翻译成内部类型。
]

== `openat`：把 Linux flags 收束成打开意图

`openat` 暴露了 ABI 边界最混杂的一类输入。它的 handler 形状本身已经体现了分层：`dirfd` 通过 `AtFd` 转成文件描述符语义，路径指针由 `c_readonly_path` copy 成内核字符串，`flags` 和 `mode` 则保留为 Linux UAPI 输入，交给 `OpenHow::from_linux()` 做集中翻译。

#listing([`openat` 的 handler 把路径 validator、dirfd conversion 和 flag translation 分开])[
```rust
#[syscall(SYS_OPENAT)]
fn sys_openat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    flags: u32,
    mode: u32,
) -> Result<u64, SysError> {
    let how = OpenHow::from_linux(flags, mode)?;
    ...
}
```
]

Linux 的 `flags` 同时混合了访问模式、路径解析要求、创建行为、fd-local flag、file status flag 和若干历史兼容位。Anemone 没有把这些位原样塞进 VFS；`OpenHow::from_linux()` 先解析并归类它们，再得到 `OpenAccessMode`、`OpenLookup`、`OpenCreate`、`FileStatusFlags`、`FdFlags`、`LinuxOpenCompat` 和 inode permission。

这种转换也承载阶段化兼容。未知 flag 直接失败；`O_PATH` 在 legacy `open/openat` 路径上按 Linux 行为取得优先级，并静默丢弃不相关 flag；`O_LARGEFILE`、`O_NOCTTY` 和 `O_ASYNC` 这类当前模型中不完整实现的兼容位，会被记录为 no-op 或 `F_GETFL` 可见状态，并打出内核日志。这里的关键不是“我们支持了多少 flag”，而是每个兼容选择都有明确位置：它属于 Linux ABI adapter，不属于 VFS object 的核心语义。

完成解析后，`sys_openat()` 才进入路径查找、创建、打开和 fd 发布。`FileDesc` 保存共享 opened file description：VFS `File` handle、访问模式、状态 flag、Linux 兼容可见位和 fd-local flag 被拆开管理。后续 `read`、`write`、`seek`、`fcntl`、`ioctl` 等路径拿到的是内部访问能力和上下文快照，而不是重新解释 `openat` 的原始 flag。

#tradeoff[
  stage-aware compatibility 允许 Anemone 接受部分 Linux 历史噪声，同时不把每个 flag 都实现成完整 Linux 内核语义。代价是边界层必须留下日志、注释和清晰的可见行为说明，否则 silent compatibility 会变成不可审查的技术债。
]

#book-figure(
  "../assets/figures/ch02/openat-flags.png",
  [`openat` 的 Linux flags 在 syscall adapter 中被拆成打开意图、fd-local flag、file status flag 和兼容可见位，而不是原样进入 VFS。],
  width: 100%,
)

== `ioctl`：控制面穿过 VFS，解释权回到 owner

`ioctl` 看起来像 syscall 层的反面：它把大量设备私有协议塞进一个 `cmd + arg` 的入口里，很难在 syscall adapter 中提前理解全部语义。Anemone 的做法是只在边界层完成通用工作：解析 fd，拒绝 path-only 目标，构造 `IoctlFileAccess`，保留用户空间访问 handle，并提供 fd-argument lookup helper。随后 `IoctlCtx` 被交给 `vfs_file.ioctl(ctx)`。

这个 context 是窄接口。它不把当前 task、fd table 或 `FileDesc` 整包暴露给后端；字符设备拿到 `CharIoctlCtx`，块设备拿到 `BlockIoctlCtx`，它们只能看到命令号、参数、目标访问能力、用户内存访问和必要的 fd lookup 能力；块设备额外接收 block owner 注入的 I/O 序列化能力。真实解释权仍在 file / device owner 手里。

这也解释了为什么 `ioctl` 同时属于 §2 和后面的设备章节。§2 关心的是 Linux 控制面如何跨过 syscall boundary 而不污染 fd/VFS 层；设备章节关心的是具体 driver owner 如何解释私有命令、保护自己的状态和返回 Linux 可见结果。

== 边界带来的收益与代价

这样的 syscall 层买到的是局部性。Linux ABI 兼容的数字、flag、errno、用户指针和历史角落集中在 adapter 附近；内部对象可以用更窄、更 Rust-friendly 的类型表达状态所有权。`Fd` 的 raw 值在边界层变成 `Fd` 类型，用户路径先通过 `c_readonly_path` 变成 `Box<str>`，文件访问模式变成 `OpenAccessMode`，`ioctl` 的 fd 权限变成 `IoctlFileAccess`。这些都不是形式主义，它们减少了跨层误用的机会。

代价也很真实。边界层会变厚：它必须理解 Linux 的历史兼容行为，也必须决定哪些行为当前实现、哪些行为作为 no-op 兼容、哪些行为直接拒绝。Anemone 选择把这层厚度留在 syscall adapter 和少数 UAPI 翻译点，而不是摊薄到每个 owner 内部。内核里没有免费的 ABI 午餐；区别只在于账单写在哪里。

本章暂时不列完整 syscall 表，也不展开完整 errno matrix。后续章节会不断回到同一原则：Linux-visible surface 是 Anemone 必须认真对待的外部契约；内部 owner boundary 则决定了这个契约怎样被消化，而不是怎样被照搬。
