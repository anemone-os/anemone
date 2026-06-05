# FileOps seek 与字符设备 ioctl 不变量需求

**状态：** Draft
**最后更新：** 2026-06-05
**父 RFC：** [RFC-20260605-fileops-seek-char-ioctl](./index.md)

## 闭合条件

- `lseek(2)`、positioned I/O 和 ioctl 分发各自有单一所有者，不能继续通过 `validate_seek(pos)` 共享一个模糊入口。
- fd-local 语义、VFS object 语义和 backend offset I/O 能力必须分层表达，并且 review 时能从 API 边界直接看出来。
- loop backing file 必须通过 VFS/fd 边界产出的窄 capability 保存，不能在 loop 子系统内保存任意 fd/file 后再推断其 positioned I/O 能力。
- 字符设备 seek 和 ioctl policy 归 `CharDev` 所有，但 `CharDev` 只能看到窄 ctx，不能默认接触完整 `File`、`FileDesc`、task 或 fd table。
- 每个迁移 gate 都必须可单独构建、审计和回滚；shared `FileOps` vtable sweep 不能和所有 ABI 语义变化压在同一个阶段里。

## 非目标

- 不引入完整 Linux `FMODE_*` 能力模型。
- 不把 loop backing file 扩展到所有 Linux 支持的对象类型；第一阶段可以保守地只接受 regular file 或已由窄 handle 明确声明支持 positioned I/O 的对象。
- 不让字符设备在本轮获得完整 tty、random、serial ioctl 协议。
- 不重写顺序 `read` / `write` 的长期 position lock 设计。

## 状态所有权

### fd / `ProcFile` / `FileDesc`

fd 层拥有 descriptor-local Linux-visible 状态：

- read/write access；
- path-only 检查；
- status flags snapshot，包括 `O_APPEND`、`O_NONBLOCK`、`O_DIRECT` 等；
- `pwrite` 遇到 `O_APPEND` 时是否走 append 路径的决策。

`FileDesc::{read_at,write_at}` 只能在完成 fd-local gate 后调用 `File::{read_at,write_at}` 或 append helper。backend 不得重复解释 fd access、path-only 或 `O_APPEND`。

### VFS opened file / `File`

`File` 层拥有 opened file object 的共享状态和 VFS-wide gate：

- `File.pos` 及其锁；
- zero-length I/O fast path；
- regular content 的只读 mount 检查；
- successful write 后的 inode metadata update；
- directory cursor rewind 的最小语义；
- `SEEK_CUR` 对当前 `File.pos` 的原子读改写。

`File::{read_at,write_at}` 在完成 VFS gate 后委托 `FileOps::{read_at,write_at}`。它不得重新调用 `seek` 或遗留 `validate_seek`，也不得通过通用 dummy cursor 把 positioned I/O 偷换成普通 `read` / `write`。

### backend / `FileOps`

`FileOps` 只表达具体文件类型的能力：

- `seek` 接收 seek intent，计算并返回用户可见的新 position；
- `read_at` / `write_at` 表达 backend 是否支持 offset-addressed I/O；
- pipe、stream char device 和未知 stream backend 默认返回 `IllegalSeek` / `ESPIPE` 风格错误；
- regular file、block device、procfs snapshot 等可 seek 或可 offset-addressed 的对象在自己的实现内做 bounds、alignment、overflow 和局部 cursor 复用。

backend 不得读取 fd table、task file table、`FileDesc` 或 fd status flags。需要 fd 状态时，fd 层必须先把它折成值类型 snapshot 或窄 capability。

### loop backing file

loop 子系统只拥有 loop block device 状态：bound/unbound、offset、size limit、readonly、flags 和展示名。它不拥有 fd permission 判断、path-only 判断或 arbitrary backend capability 推断。

`LOOP_SET_FD` 必须通过 VFS/fd 边界产出 `BackingFileHandle` 或等价窄类型：

- conversion 阶段验证 fd access、path-only、readonly、file type 或 positioned I/O capability；
- handle 内部可以包住 `Arc<File>`，但对 loop 只暴露 `read_exact_at`、`write_all_at`、`visible_size`、`get_attr`、`display_name` 等必要操作；
- loop state 保存 narrowed handle，而不是任意 `Arc<File>` / `FileDesc` / `IoctlArgFile`；
- 未来若放宽到 block device 或其他 positioned-capable backend，只能扩展 handle conversion contract，不能让 loop 直接理解每种 VFS backend。

### character device

字符设备 policy 归 `CharDev` trait 所有。统一 char devfs file ops 只做：

- 从 `rdev` 找到 `CharDev`；
- 构造窄 ctx；
- 调用 trait hook。

`CharDev::seek` 使用 `CharSeekCtx` 或等价窄参数，默认返回 `IllegalSeek`。ctx 第一阶段只应包含 seek intent 和由当前 `File::seek` guard 派生的短生命周期 position cursor；除非后续 RFC 证明必要，不加入 `&File`、`FileDesc`、task、fd table 或完整 path/inode 访问。

`CharDev::ioctl` 使用 `CharIoctlCtx`，作为 `IoctlCtx` 的窄包装或 type alias。它可以复用 user pointer copy 和 fd-arg lookup snapshot 能力，但不能暴露 `FileDesc` 或当前 task。

## 身份与能力模型

- `SeekFrom` 是 seek intent，不是最终 offset。`sys_lseek()` 只负责 Linux whence 转换，最终 position 由 opened file 的 `seek` 实现决定。
- `SEEK_DATA` / `SEEK_HOLE` 在第一阶段不是 backend seek intent。whence 转换层或等价前置 gate 必须返回明确 unsupported / NYI，不能把它们吞成普通 `EINVAL`，也不能让 backend 在没有完整语义时自行猜测。
- `IoctlFileAccess` / status flag snapshot 是 fd capability 的值类型表示，允许向 file ops 或 device ioctl 传递，不允许把 `FileDesc` 直接传下去。
- `BackingFileHandle` 是 loop backing 的唯一长期 capability。它声明的是“这个对象已通过 positioned I/O backing 校验”，不是“这个 file ops 结构里存在 `read_at` 字段”。
- `CharSeekCtx` 和 `CharIoctlCtx` 是字符设备看到 VFS 请求的唯一默认窗口；ctx 字段应按真实需求逐项加入。`CharSeekCtx` 中的 position cursor 必须是从当前 `File::seek` guard 派生的短生命周期 `&mut pos` 或等价能力，只能在调用期间更新，不能被 `CharDev` 保存或转交给异步路径。

## 线性化点

- `File::seek` 在线性化点上持有 `File.pos` lock，读取旧 position、调用 backend seek、写入新 position 并返回用户可见值。`SEEK_CUR` 不得在 syscall 层先读 `pos()` 再独立 `seek()`。
- 字符设备 seek 的 cursor 更新必须发生在同一个 `File.pos` guard 覆盖的线性化点内；memory char device 的 null-style seek 只能把该短生命周期 cursor 设回 `0` 并返回 `0`。
- positioned `read_at` / `write_at` 不读取、不写入普通 `File.pos`。局部 cursor 只能在具体 backend 内部存在。
- `FileDesc::write_at` 对 `O_APPEND` 的选择发生在 fd 层；进入 backend 后请求已经是普通 positioned write 或 append helper，不再携带 fd-local policy。
- `LOOP_SET_FD` 的可见状态变化发生在 `LoopState` lock 下发布 narrowed handle 时。转换失败不得改变 loop state。

## 锁序与生命周期规则

- fd lookup 和 capability snapshot 不得把 fd table guard 带入 device backend。
- loop state 不保存 `FileDesc`，避免 descriptor lifetime、fd flag mutation 和 device lifetime 绑在一起。
- `CharDev` hook 不得持有 char registry lock 后回调需要重新进入 registry 的路径；统一 devfs file ops 应先完成 lookup，再调用具体设备。
- `File.pos` lock 只保护 opened file cursor，不作为 backend I/O 的全局序列化证明。具体 backend 如需额外序列化，应使用自己的锁。

## 禁止退化项

- 不得把 `validate_seek(pos)` 保留为 `FileOps` 字段或作为 `lseek` 抽象。
- 不得建立跨所有文件类型的通用 `read_at` / `write_at` dummy-cursor wrapper。
- 不得让 pipe 或 stream char device 在 backend 不知情时继承 positioned I/O。
- 不得让 `sys_lseek()` 继续用 `inode().size()` 解释所有 `SEEK_END`。
- 不得把 memory char device 的 seek 特例硬编码在统一 devfs file ops 中。
- 不得把完整 `File` / `FileDesc` 作为 `CharDev::seek` 的默认参数。
- 不得让 loop 保存任意 `Arc<File>` 后在 block I/O 路径才发现 positioned I/O 不支持。

## 完成标准

- `tracking-issues.md` 中当前 Keter / Euclid 项都能指向本文或 [迁移实施计划](./implementation.md) 的 canonical 修复文本。
- 阶段 1A、1B、1C 可以分别用搜索审计和 `just build` 证明闭合。
- 阶段 1A 只能证明 shared vtable 构建闭合和 fail-closed 默认形状，不声明 `lseek` 或 positioned I/O 外部语义完成；阶段 1B/1C 才分别作为用户可见 seek 与 positioned I/O 的语义验收 gate。
- 代码中任何剩余 offset helper 都是具体 backend 内部 helper，不再承担跨 `lseek` 与 positioned I/O 的 shared contract。
- loop backing、char seek 和 char ioctl 都通过窄 API 边界表达所有权。
