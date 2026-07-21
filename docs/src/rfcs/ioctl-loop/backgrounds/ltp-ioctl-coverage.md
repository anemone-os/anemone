# LTP IOCTL 测例覆盖面

本文记录本仓库 `ioctl` LTP 分组主要在测什么，以及这些测例对
[RFC-20260603-IOCTL-LOOP](../index.md) 的阶段划分有什么影响。Canonical 范围仍以
RFC 入口、[不变量需求](../invariants.md) 和 [迁移实施计划](../implementation.md) 为准；
本文只作为背景材料，避免把 LTP 中所有 ioctl 子系统一次性并入 loop 第一阶段。

## 来源

本仓库启用的 ioctl 分组来自
`anemone-apps/user-test/ltp/groups/ioctl.txt`，当前包含：

- `ioctl01..ioctl09`
- `ioctl_loop01..ioctl_loop07`
- `ioctl_ns01..ioctl_ns07`
- `ioctl_sg01`
- `sockioctl01` 在该分组中被注释，不计入本 RFC 的当前 ioctl 分组背景。

当前可复核的测例说明和源码来自初赛 testsuite 中的 vendored LTP：

- `<preliminary-testsuite>/ltp-full-20240524/runtest/syscalls`
- `<preliminary-testsuite>/ltp-full-20240524/testcases/kernel/syscalls/ioctl/`

## 总体判断

LTP 的 ioctl 分组不是单纯测试 `ioctl(2)` 这个 syscall 入口，而是把多个设备和文件
子系统的 ioctl 协议放在同一个 runtest 段中：

1. syscall/VFS 分发和通用 errno 边界；
2. tty、TUN/TAP、random、block、btrfs、loop、namespace、SCSI generic 等子系统私有协议；
3. LTP 测试设备基础设施依赖的块设备与 loop 设备能力；
4. sysfs、`/proc`、mount、partition reread、namespace clone 等配套可观测性。

因此，`ioctl_loop` RFC 第一阶段应优先闭合 “VFS ioctl 分发 + 通用 block size ioctl +
loop 绑定/状态/释放最小闭环”。LTP ioctl 全量绿灯还需要 namespace、btrfs、TUN/TAP、
SCSI generic、sysfs loop 属性、partscan、direct I/O、read-ahead、read-only block
状态等后续能力，不能作为第一阶段验收口径。

## 基础 ioctl 测例

| 测例 | 主要覆盖 | 对本 RFC 的含义 |
| --- | --- | --- |
| `ioctl01` | `EBADF`、用户指针 `EFAULT`、未知命令在 tty/普通文件上的 `ENOTTY`，请求包含 `TCGETA` / `TCGETS`。 | 约束 `sys_ioctl()` 的最外层 fd lookup、用户指针边界和默认 unsupported errno；普通文件未知 ioctl 应稳定返回 `ENOTTY`，不能落到 `ENOSYS`。 |
| `ioctl02` | tty `TCGETA` / `TCGETS` 与 `TCSETA` / `TCSETS`，检查 termio/termios 参数能写回再读出。 | 属于 tty 驱动私有 ioctl，不是 loop 第一阶段；但它要求 VFS ioctl 分发能把命令送到打开文件对应的 file ops。 |
| `ioctl03` | `/dev/net/tun` 或 `/dev/tun` 的 `TUNGETFEATURES`，枚举 TUN/TAP feature flags。 | 依赖 TUN/TAP 字符设备；不属于 ioctl-loop RFC 第一阶段。 |
| `ioctl04` | 块设备 `BLKROGET` / `BLKROSET`，并用 read-only / read-write mount 验证只读状态。 | 是后续块设备状态 ioctl 与 mount errno 的结合测例；第一阶段只要求 size 类 block ioctl，不应把 `BLKRO*` 提前变成验收阻塞项。 |
| `ioctl05` | `BLKGETSIZE` 与 `BLKGETSIZE64` 一致性，设备末尾 `lseek` 和越界 `read` EOF。 | 直接支撑本 RFC 的通用 block size ioctl 目标，是第一阶段最相关的基础测例。 |
| `ioctl06` | `BLKRASET` / `BLKRAGET` read-ahead 设置和读回。 | 块设备调优状态，不属于最小 loop/mount 闭环。 |
| `ioctl07` | `/dev/urandom` 的 `RNDGETENTCNT` 与 `/proc/sys/kernel/random/entropy_avail` 对比。 | random 字符设备和 procfs 可观测性，不属于 ioctl-loop RFC。 |
| `ioctl08` | btrfs `FIDEDUPERANGE` 文件范围去重，包括 same/diff/invalid length。 | btrfs filesystem ioctl；当前 RFC 不覆盖 btrfs。 |
| `ioctl09` | `BLKRRPART`，语义等同 `blockdev --rereadpt`，并依赖 loop driver 与 `parted`。 | 属于 partition reread / partscan 后续范围；不能用来要求第一阶段生成 loop 分区设备。 |

## loop ioctl 测例

`ioctl_loop01..07` 不是只检查 “能不能把文件绑定成 loop 设备”。它们覆盖 Linux loop
driver 较宽的状态面，并大量使用 `/sys/block/loop*` 作为 oracle：

| 测例 | 主要覆盖 | 对本 RFC 的含义 |
| --- | --- | --- |
| `ioctl_loop01` | `LO_FLAGS_AUTOCLEAR`、`LO_FLAGS_PARTSCAN`、`LOOP_SET_STATUS` / `LOOP_GET_STATUS`、`/sys/block/loop*/loop/{autoclear,partscan,backing_file}`、分区节点 `/dev/loopNp1`。 | 可作为后续 autoclear/sysfs/partscan 参考；第一阶段允许支持 autoclear 最小语义，但 `PARTSCAN` 和 loop 分区设备生成仍是非目标。 |
| `ioctl_loop02` | `LO_FLAGS_READ_ONLY`、`LOOP_SET_FD`、`LOOP_CONFIGURE`、`LOOP_GET_STATUS`、只读写入失败、`LOOP_CHANGE_FD`，并检查 sysfs `ro` 和 `backing_file`。 | `LOOP_SET_FD`、readonly flag 和 status 读回与本 RFC 相关；`LOOP_CONFIGURE`、`LOOP_CHANGE_FD` 和 sysfs 完整性属于后续扩展或单独验收。 |
| `ioctl_loop03` | 非 read-only loop 上 `LOOP_CHANGE_FD` 应以 `EINVAL` 失败。 | `LOOP_CHANGE_FD` 不是第一阶段 loop 闭环。 |
| `ioctl_loop04` | backing file 扩容后 `LOOP_SET_CAPACITY` 更新 live loop size，并检查 sysfs size。 | live resize 与 sysfs size 是后续范围。 |
| `ioctl_loop05` | `LOOP_SET_DIRECT_IO`、direct I/O flag 读回、offset 对齐、`BLKSSZGET`、`LOOP_SET_BLOCK_SIZE`。 | `BLKSSZGET` 对通用块设备有价值；direct I/O 和 block size 调整不是第一阶段目标，应稳定拒绝而不是伪成功。 |
| `ioctl_loop06` | `LOOP_SET_BLOCK_SIZE` 与 `LOOP_CONFIGURE.block_size` 的非法值：小于 512、大于 page size、非 2 的幂。 | 如果尚未支持可变 loop block size，应返回稳定 unsupported；若后续支持，必须先固定这些 `EINVAL` 边界。 |
| `ioctl_loop07` | `LOOP_SET_STATUS64` / `LOOP_GET_STATUS64` 和 `LOOP_CONFIGURE` 的 `lo_sizelimit`，并检查 sysfs `size` / `sizelimit`。 | `sizelimit` 是当前 RFC loop 状态的一部分；但 LTP 用 sysfs 断言外部可观测性，第一阶段若不实现 sysfs 仍不能承诺该测例完整通过。 |

对当前 RFC 最有用的 loop 结论是：

- 空闲 loop discovery 依赖 `/dev/loopN` 和 `LOOP_GET_STATUS*` 的可分类错误；半发布
  `/dev/loop-control` 会改变 discovery 路径，因此第一阶段不发布该节点。
- `LOOP_SET_FD` 的 fd 参数只是一瞬时输入；loop 设备成功绑定后必须保存独立的 backing
  file handle，而不是保存 raw fd number。
- `LOOP_GET_STATUS*` / `LOOP_SET_STATUS*` 需要把 UAPI 结构体转换为内部 loop 状态；长期
  状态不应直接保存 Linux `loop_info` / `loop_info64`。
- unsupported flags 和后续 ioctl 必须稳定失败，例如 `PARTSCAN`、`DIRECT_IO`、
  `SET_BLOCK_SIZE`、`CHANGE_FD`、`SET_CAPACITY`；不能把 flag 记录下来制造假进展。

## namespace ioctl 测例

`ioctl_ns01..07` 测的是 `/proc/<pid>/ns/*` namespace 文件上的 `NS_GET_*` ioctl，而不是
loop/block ioctl：

| 测例 | 主要覆盖 | 对本 RFC 的含义 |
| --- | --- | --- |
| `ioctl_ns01` | `NS_GET_PARENT`：initial namespace 或新 pid namespace 的 parent 查询按权限返回 `EPERM`。 | namespace 层语义，不属于 loop 第一阶段。 |
| `ioctl_ns02` | 对 UTS namespace 执行 `NS_GET_PARENT` 返回 `EINVAL`，因为它不是层级 namespace。 | namespace 类型边界。 |
| `ioctl_ns03` | 对非 user namespace 执行 `NS_GET_OWNER_UID` 返回 `EINVAL`。 | namespace 类型边界。 |
| `ioctl_ns04` | `NS_GET_USERNS` 的 owning user namespace 超出调用者作用域时返回 `EPERM`。 | namespace 权限边界。 |
| `ioctl_ns05` | `CLONE_NEWPID` 后检查 child pid namespace、child pid 为 1，并测试 parent 查询。 | 依赖真实 pid namespace clone 语义。 |
| `ioctl_ns06` | `CLONE_NEWUSER` 后检查 child user namespace，并测试 `NS_GET_USERNS`。 | 依赖真实 user namespace clone 与权限语义。 |
| `ioctl_ns07` | 对普通目录 fd 执行 `NS_GET_*` 返回 `ENOTTY`。 | 对本 RFC 有一个通用启示：非目标 file type 的未知 ioctl 应由统一分发返回 `ENOTTY`。 |

这些测例可以作为未来 namespace file ops ioctl 的参考，但不应阻塞 ioctl-loop 的 VFS/block/loop
阶段。

## SCSI generic 与未启用 sock ioctl

`ioctl_sg01` 是 `SG_IO` 的 CVE 回归测例，要求可读的 generic SCSI 设备，例如 `/dev/sg*`，
并检查内核不会泄露未初始化数据。它属于 SCSI generic 字符设备和内存初始化边界，不属于
loop/block 第一阶段。

`sockioctl01` 在 upstream LTP `runtest/syscalls` 中存在，但本仓库 `ioctl` 分组将它注释掉。
因此本 RFC 背景不把 socket ioctl 作为当前分组验收内容；后续如果启用，需要单独从 socket
file ops 和 net-device ioctl 边界评估。

## 对 RFC 阶段的落点

第一阶段建议只把下列 LTP 压力面纳入直接验收解释：

- `ioctl01` 中 fd lookup、用户指针、未知命令和普通文件 `ENOTTY` 的通用边界；
- `ioctl05` 中 `BLKGETSIZE` / `BLKGETSIZE64` 及块设备末尾访问语义；
- LTP 测试基础设施需要的 loop discovery、`LOOP_SET_FD`、`LOOP_CLR_FD`、
  `LOOP_GET_STATUS*`、`LOOP_SET_STATUS*` 的最小状态闭环；
- `BLKSSZGET`，主要服务块设备工具链和后续 loop direct I/O / block size 测例的前置能力。

以下内容应明确归为后续或非目标，除非另开 follow-up RFC：

- tty、TUN/TAP、random、btrfs、namespace、SCSI generic、socket ioctl；
- `BLKRO*`、`BLKRA*`、`BLKRRPART` 等更宽的 block ioctl；
- `/sys/block/loop*` 完整属性、uevent、partition scan 和 `/dev/loopNpM`；
- `LOOP_CONFIGURE`、`LOOP_CHANGE_FD`、`LOOP_SET_CAPACITY`、`LOOP_SET_BLOCK_SIZE`、
  `LOOP_SET_DIRECT_IO` 的完整 Linux 语义。

换句话说，LTP ioctl 分组说明了 ioctl 分发必须是 VFS/file-ops 可扩展架构，而不是
`sys_ioctl()` 内的一串全局特判；但它也说明 ioctl-loop 第一阶段需要坚持范围控制，把
不同设备协议留给各自后续实现。
