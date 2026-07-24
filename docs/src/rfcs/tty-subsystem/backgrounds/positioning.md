# TTY 子系统定位

**状态：** 已归档的 pre-RFC 共识稿。本文记录 TTY 在设备架构中的前置定位，不再维护，也不覆盖 [RFC target](../index.md)、实现计划、current contract 或代码改动授权。

## 1. 目的与证据

在设计 termios、line discipline、PTY 或 terminal job control 之前，先回答两个更基础的问题：TTY 是否属于 `char`，以及一个物理设备是否可以同时向多个设备子系统注册。

本文依据以下现状与参考调查：

- [Anemone 当前边界](./current-state.md)
- [PulseOS、MyGO 与 KernelX 的 TTY 支持调查](./three-reference-kernels-tty-survey.md)
- [设备号 ownership 收口记录](../../../devlog/changes/2026-07-22-device-devnum-ownership.md)
- 仓库内 Linux 6.6.32 参考源码，主要包括 `fs/char_dev.c`、`drivers/tty/`、`drivers/rtc/` 和 `drivers/input/`

这里讨论的是长期架构方向，不意味着第一阶段必须同时实现所有能力。

## 2. 基本模型：按领域能力划分并列子系统

`char`、`block`、`tty`，以及未来可能出现的 `rtc`、`input` 等，应当是 `device/` 下按领域能力划分的并列子系统，而不是按“最终是否表现为字符特殊文件”建立父子关系。

每个子系统可以拥有自己的：

- 注册接口和 registry；
- 注册者必须提供的领域 vtable / capability；
- 领域对象、协议状态及其生命周期；
- 面向 VFS 的统一 open、`FileOps` 和每次 open 状态；
- devfs 命名、属性、发布和撤销策略。

这里的“注册者”不必永远等同于物理驱动。它可以是物理驱动本身，也可以是持有物理驱动窄 capability 的适配层或 frontend。关键约束是：registry 保存 capability handle 和发布元数据，不复制其背后的物理状态或协议真相。

因此，“设备最终位于 block/char 两个特殊文件编号空间之一”只描述用户 ABI 和 VFS 节点形态，不决定内核中的语义 owner。TTY 在 devfs 中通常表现为字符特殊文件，但这不使 TTY 成为 `char` 的子模块。

## 3. 分层与 owner 边界

| 层次 | 拥有的内容 | 不应拥有的内容 |
| --- | --- | --- |
| 物理驱动 | MMIO、IRQ、DMA、硬件 FIFO/队列、线路配置、硬件错误、同一物理资源的序列化 | termios 真相、line discipline、controlling TTY、通用 TTY open 语义 |
| 领域子系统 / frontend | 领域 registry、领域对象和协议状态、领域 vtable、endpoint identity/name 映射策略、统一 `FileOps`、devfs 发布生命周期 | 具体 UART 寄存器、另一子系统的协议真相、物理资源的第二份 owner |
| `device::devnum` | block/char 设备号的值类型、静态 namespace entry 和可复用的 minor allocator 机制 | 具体映射策略、allocator 实例状态、char、TTY、RTC 等领域行为 |
| devfs | 节点、inode 属性、名称和已发布 open provider 的保存与分发 | 设备协议、registry 真相、硬件生命周期 |
| VFS / opened file | 通用文件标志和每次 open 的文件状态，把操作分发到发布者提供的 `FileOps` | 通过设备类型猜测 TTY 策略 |
| task / unix-jobctl | session/process-group 拓扑、signal 投递和 ThreadGroup stop/continue 真相 | termios、输入队列和 line discipline |

`char` 仍是一个真实的基础字节设备子系统：其 registry、`CharDev` 能力和统一文件行为服务于 `/dev/null`、`/dev/zero`、random 设备以及确实只需要基础字节接口的设备。它不是单纯的设备号分配器，也不是所有字符特殊文件语义的总 owner。

反过来，设备号和 devfs 是各领域子系统可复用的中立基础设施。当前 devfs 节点已经保存自己的 `DevfsNodeOps`，足以让 `char`、`block` 和未来的 `tty` 分别发布自身 open provider；现阶段不需要先建立一个模仿 Linux cdev 的全局动态解析层。

### 3.1 静态设备号分区

Anemone 当前是封闭开发模型：可用子系统、内建驱动和目标 Linux ABI 都由仓库统一控制，没有 loadable module 或第三方子系统在运行时申请未知编号。因此跨子系统设备号不需要动态分发；`device::devnum` 已经静态列出各领域使用的 namespace entry。char 与 block 是两个独立 namespace，编号唯一性分别在各自 namespace 内成立，同一个 numeric major 可以在两者中同时存在。

静态 entry 属于用户可见的领域发布面，不机械对应物理驱动。当前字符设备 namespace 已为 TTY 保留 major 4，并让现有 raw serial endpoint 使用 major 234；后者是当前 raw-char 投影，不是 TTY identity。未来支持 TTY 的 NS16550A、PL011 等物理驱动向 TTY 提供 `TtyPort`，由 TTY 领域的映射策略在 major 4 内为 terminal endpoint 构造 minor 和 canonical name，而不是让每个物理驱动各占一个 major。RTC 或 input 也可以分别拥有自己的固定 major/minor entry。

设备号 ownership 因而分为三层：

| 范围 | 策略 | Owner |
| --- | --- | --- |
| 跨领域子系统 | 在 `device::devnum` 中静态编码；char、block 各自在自己的 namespace 内保持唯一 | 设备号 namespace 规则 |
| 某个静态 entry 内部 | 固定映射或由领域 owner 持有的 allocator 生成逻辑实例编号，并据此构造 endpoint identity/name | char、TTY、RTC、input 等领域 core、frontend 或 producer |
| endpoint 注册 | 从 capability 的不可变 identity 派生 registry key，并校验 devnum/name 唯一性 | 对应领域 registry |

`GeneralMinorAllocator` 一类 helper 只提供分配机制，不拥有任何领域的分配状态或映射策略。这套边界不要求增加运行时全局 region allocator、region handle 或通用注册事务。启动期 probe 按当前平台假设通常不会失败；第一版只需在自然 owner 边界内尽可能撤销已经完成的本地步骤，不为低概率失败路径引入大型跨子系统事务。只有将来引入动态模块、第三方子系统、运行期 hotplug 或其它无法预先枚举的发布域时，才重新评估这些机制。

如果未来支持用户通过 `mknod` 任意创建字符或块设备节点，需要补充中立的 `DeviceId -> open provider` 解析；设备号归属和不重叠规则仍由静态 namespace contract 决定，不必因此引入动态子系统编号分配。该问题不应提前反向塑造第一版 TTY registry。

## 4. TTY 与 char 的关系

TTY 不应实现为通用 `CharDev` 的一个大号实例，也不应为了容纳 TTY 而把 `CharDev` 扩展成可观察完整 task、session、fd table 和每次 open 状态的宽接口。

原因不是 TTY “不属于字符设备号空间”，而是两者要求注册者提供的能力和子系统自己承担的语义不同：

- `CharDev` 面向基础字节 read/write，以及设备私有的窄 seek/ioctl；
- TTY 需要每个 terminal 共享的 termios、line discipline、输入/输出队列、hangup 和 foreground process group；
- TTY 的 blocking/nonblocking、poll、ioctl 和 controlling-terminal 语义需要专属 `FileOps` 及 caller/open 上下文；
- 同一 TTY 的多个 open 必须汇合到同一 terminal 语义对象，同时保留必要的 per-open 状态。

建议的模块位置是 `device::tty`。串口驱动只向它提供类似 `TtyPort` 的窄后端 capability；TTY core 创建并拥有 terminal 对象，通过自己的 registry 和 devfs bridge 发布 `/dev/ttyS*` 等节点。依赖方向应当是：

```text
serial driver -> device::tty backend contract
device::tty    -> static devnum contract / devfs / wait and narrow task capabilities
VFS open       -> TTY-owned FileOps -> terminal object -> TtyPort
```

`device::tty` 不依赖具体 UART 类型，也不经由 `device::char::CharDev` 转发数据路径。TTY core 或其 frontend 在静态分配给 TTY 的字符设备号 entry 内构造 terminal endpoint 的不可变 devnum/name，TTY registry 从 endpoint identity 派生 key，只负责唯一性校验、保存和查询。TTY 与 char 共同使用字符设备号和 devfs 字符节点形态，但不共享领域 registry。

## 5. 一个物理设备可以投影到多个子系统

物理驱动唯一拥有底层资源，并负责决定从同一物理状态构造哪些窄 capability、把它们注册到哪些子系统。注册本身不是资源复制：各 registry 只持有指向同一物理 owner 的能力句柄。

当前 NS16550A state 已经同时提供 console capability 和 major 234 下的 raw `CharDev` capability。这证明同一物理 owner 可以投影多个窄能力，但不证明未来 raw-char、TTY 与 console 可以不经仲裁地同时活跃。

框架只定义各领域 capability 的注册和调用 contract，不建立通用的 personality manager，也不尝试发现两个 registry entry 是否共享同一硬件。char 只理解 `CharDev`，TTY 只理解 `TtyPort`，console 只理解 console backend；多个能力如何组合、互斥或同时激活，是物理驱动自己的协议状态。

驱动负责跨 capability 的硬件序列化，但“加一把锁”不足以定义多投影语义。驱动为每组并存关系还必须明确：

- 谁消费 RX，字节是独占、复制、tap 还是按协议 demux；
- 谁拥有 baud rate、flow control 等硬件配置；
- TX 是否允许合流，输出之间是否需要 framing 或优先级；
- 某个 frontend attach/detach、open/close 或 hangup 时，谁改变硬件状态；
- 一个投影失败或注销时，是否影响其它投影。

至少要区分以下情形：

1. **可共享但需要仲裁。** 例如 bootstrap console 和 TTY 可以复用 UART TX，但必须由物理 owner 提供统一 TX serialization，并处理 IRQ 上下文中的 printk 递归风险。
2. **明确的 observer/tap。** 某个消费者只观察一份复制流，不取得输入消费或配置权。
3. **由物理层 demux 的独立通道。** 只有硬件或协议确实提供可分离通道时，多个 frontend 才能各自拥有输入。
4. **互斥能力。** raw-char、TTY 或其它串口协议若竞争同一 RX 和线路配置，驱动应选择只注册其中一种，或通过 driver-local claim/lease 在 bind、open 或激活阶段明确互斥，而不是让两个子系统同时读写后再依靠锁碰运气。

因此，一个 UART 可以同时向 console 和 TTY 提供能力；也可以在驱动内部定义明确共存规则后同时发布 raw-char 和 TTY。但“同一驱动能注册多个子系统”不自动保证这些用户接口可以同时活跃。注册与激活应当区分：领域子系统可以保存并发布 capability，真正触及共享硬件时仍由驱动验证当前 claim 和配置。

这里的 personality 只适合作为讨论“同一硬件的不同高层用途”的描述词，不预设一个长期类型或通用状态机。若实际约束只是一条共享 TX 路径，就只需要 driver-owned TX serialization；若约束只是唯一 RX consumer，就只建立对应的 driver-local claim。只有硬件确实存在整体模式切换时，驱动才需要显式模式状态机。

## 6. TTY core 的责任边界

TTY core 预期拥有：

- terminal object 及其生命周期；
- termios、winsize 和 line discipline；
- cooked/raw 输入、echo、输出变换和队列语义；
- blocking/nonblocking、poll/readiness 和 TTY ioctl；
- open/close、hangup 以及后续 PTY 共享的终端语义；
- controlling TTY、foreground/background access 检查及 terminal-generated signal 的策略。

物理 UART backend 预期只提供原始传输和硬件控制能力，例如 RX drain/notification、TX 提交/readiness、线路参数设置、flush 和错误报告。具体方法集合仍由后续 RFC 决定。

terminal job control 必须接入现有 unix-jobctl，而不是在 TTY 内建立第二套 stop 状态机。TTY 负责确认 controlling terminal 和 foreground/background 关系，并向准确的 process group 生成 `SIGINT`、`SIGQUIT`、`SIGTSTP`、`SIGTTIN`、`SIGTTOU` 或 `SIGHUP`；之后由 signal 与 ThreadGroup jobctl owner 完成 stop/continue 和 wait report。

## 7. Linux 提供的架构启发

Linux 支持上述总体方向，但它给出的更重要启发是 owner 分离，而不是目录名或对象数量：

- `dev_t` / cdev 解决字符特殊文件的编号和 open 路由，不拥有 TTY 语义。
- TTY core 自己拥有 `tty_driver`、`tty_port`、`tty_struct`、`tty_fops` 和 `tty_class` 等对象；UART serial core 向上提供端口能力。
- RTC driver 实现 `rtc_class_ops`，RTC core 统一拥有 RTC 对象、文件操作、cdev 和节点发布。这是“领域 vtable + 领域 registry/core + 字符节点”的清晰样本。
- input core 将设备与 handler/frontend 分开；物理驱动不必亲自实现每一种用户可见 frontend，匹配后可以由子系统自动 attach。
- Linux 同时支持固定和动态设备号；RTC、input 以及许多 TTY ABI 使用固定 major 和领域内 minor 分区，而动态 region 还要服务其开放模块生态。Anemone 借鉴编号空间与语义 owner 分离，不必复制动态分配机制。
- TTY 与 serdev 的关系说明，同一串口的两种高层用途在资源和配置冲突时可以由驱动/bind 路径选择互斥，而不是由一个跨子系统框架强行同时发布。

Anemone 不应照搬 Linux 的大型 kobject/class 框架、运行时跨领域 region 分发、历史形成的 TTY backpointer 和锁关系、把宽 `file_operations` 当作物理驱动 vtable 的做法。我们只借鉴 Linux 的语义 owner 与特殊文件路由分离原则。

## 8. 已形成的定位共识

1. TTY 位于 `device::tty`，与 `device::char`、`device::block` 并列。
2. 各领域子系统可以拥有独立 registry、领域 vtable、协议状态、专属 `FileOps` 和 devfs 发布策略。
3. `device::devnum` 静态声明设备号 namespace entry；char 与 block 是独立 namespace，各自在内部保持唯一，当前不引入运行时全局 region allocator。
4. TTY 使用字符设备 major 4；当前 raw serial 使用 major 234。raw serial 是现状过渡投影，不反向定义 TTY 的 registry 或数据路径。
5. 领域 core、frontend 或 producer 在自己的静态 entry 内固定或动态生成逻辑实例编号，并在注册前构造不可变 endpoint devnum/name；registry 从 capability 派生 key，只负责唯一性校验、保存和查询。
6. major/minor 和 canonical name 表达用户 ABI endpoint，不机械映射物理驱动或物理设备。
7. 用户 ABI 中属于字符设备号空间，不等于语义上从属于 `char`；devfs 继续保存各发布者自己的 open provider。
8. `char` 继续拥有基础字节设备的真实语义，不能退化为只有设备号的空壳。
9. 物理驱动决定注册哪些领域 capability，并唯一拥有硬件资源、跨 capability 序列化、共享配置及互斥/共存协议；registry 只保存 capability，不复制真相。
10. 不建立通用 personality manager。框架只定义各领域 capability contract，驱动只为真实硬件约束建立必要的局部 claim、demux 或模式状态。
11. TTY 使用专属 open/FileOps 路径，不扩大通用 `CharDev` 的 task/session 可见面。
12. TTY 生成 terminal signal 并消费窄 task/jobctl capability；现有 unix-jobctl 继续唯一拥有 stop/continue 状态机。

## 9. 后续 RFC 必须关闭的问题

以下问题尚未由本文决定：

- 第一版 `TtyPort` backend vtable 的精确方法、上下文约束和错误模型；
- UART IRQ top half、固定容量 RX ring、deferred processing、overflow/line-error 统计分别由谁拥有；
- bootstrap console、TTY、panic/printk 共用 UART TX 时的仲裁、递归和切换规则；
- controlling TTY、session leader、foreground process group 与 TTY 生命周期之间的双向 handoff、锁顺序和 cleanup；
- VFS open 如何向 TTY 提供足够窄的 caller/session 信息，以及 `O_NOCTTY`、`/dev/tty` 和 reopen 的语义；
- TTY major 4 内的准确 minor 布局、固定编号与动态 port index 的兼容边界，以及哪个 TTY frontend 负责从 port identity 构造 endpoint identity/name；
- 当前 major 234 raw-char 投影在 TTY 接入后继续保留、与 TTY 互斥，还是在明确迁移 gate 后撤销；
- 第一阶段是否只做 serial TTY，PTY/ptmx/devpts 在何时进入，以及 serial 与 PTY 共享哪些 terminal core；
- 支持任意 `mknod` 后，中立 `DeviceId -> open provider` resolver 的最终形态。

本文只保留当时形成的 positioning 证据。当前 target、最小 current-contract 闭包、vertical slice、验证 floor 和停止条件均由[公开 RFC](../index.md)及其子文档维护；不得把本背景页重新解释为实现清单或生效契约。
