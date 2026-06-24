# Anemone Book 定位契约

本文记录 `anemone-book` 的写作定位。它不是公开 RFC，不承载内核设计
canonical contract；它是面向初赛提交的设计叙述快照和读者入口。

## 读者与目的

本文档的第一读者是初赛评审老师 / 评委，而不是 Anemone 的后续维护者，
也不是 OS 初学者。

读者读完后应形成的核心判断是：Anemone 不是为了测例堆 syscall 的兼容层，
也不是 Linux 的 Rust 复刻；它是一个有清晰 owner boundary、ABI 策略和
工程纪律的现代 OS kernel。测试和真实负载只在支撑设计判断时作为证据出现，
不是叙事主线。

这不是全书反复强调的口号，而是前言和第一章需要说清的澄清边界。全书的
主线是：Anemone 如何在 Linux ABI 兼容、Rust 内核结构、阶段化工程取舍
之间建立一套可维护的系统设计。

## 非目标

- 不写成 syscall 手册。
- 不写成源码逐文件导览。
- 不复刻 RFC 细节。
- 不列 LTP case matrix、TPASS 分数或比赛得分策略。
- 不把 register / current limitations 全量搬进正文。
- 不把 `anemone-book` 描述成 Anemone 的 single source of truth。

## Artifact 边界

- 代码是行为真相源。
- RFC 是已接受设计 contract。
- devlog / transaction devlog 记录执行事实、checkpoint、review 和验证证据。
- register / current limitations 记录当前开放问题和接受限制。
- `anemone-book` 负责把这些事实组织成适合评委阅读的设计叙述。

早期私人讨论材料已经吸收进本契约，不作为公开书稿的稳定依赖。正式书稿引用
设计事实时，应引用已有 RFC、devlog、register、current limitations 或代码，
而不是引用私人草稿路径。

## 叙述主线

全书采用故事线优先的设计叙述，按系统设计主题组织，必要时再映射到源码
模块。章节结构优先回答系统能力和设计取舍，而不是复现目录树。

第一章承担定调和架构地图职责：说明 Anemone 的目标、非目标、Linux ABI
compatibility surface 与内部 native contract、主要子系统地图，以及 owner
boundary、single source of truth、stage-aware compatibility、observability
等全局设计原则。

正文采用“章节主线 + 模块覆盖矩阵”的双层结构。正文按设计主题推进；
`outline.md` 维护覆盖矩阵，保证 task、scheduler、time、mm、VFS、fs、
procfs、devfs、driver model、arch、syscall、signal、IPC 等重要模块都有
明确落点。

优先主线包括：

- Linux-visible compatibility surface 与 Anemone-native internal contract 的分层。
- 任务、调度、等待、signal 和时间路径如何共享状态所有权与唤醒协议。
- VFS、mount tree、procfs、devfs 和未来 sysfs 如何把内核对象接入 namespace
  与 control surface，同时不接管对象状态所有权。
- 设备驱动模型如何独立于 VFS，并通过 devfs 等桥接层暴露给用户态。
- 内存管理、page cache、mmap、SysV shm 等路径如何表达资源所有权和阶段性承诺。
- arch、trap、syscall dispatch、interrupt、FPU/context 与 unsafe assembly
  如何形成 user/kernel boundary。

## 深度边界

每个核心子系统章讲到：

- 子系统职责；
- 核心状态的 owner 和不变量；
- 代表性 data path / control path；
- 关键 trade-off；
- 当前承诺与明确不承诺的边界。

正文不展开逐函数实现、完整 errno matrix、完整测例失败矩阵或 RFC 全文压缩版。

不是“实现过就必须写”。只有当一个功能能解释核心设计原则、关键 trade-off、
Linux ABI 与内部结构的分层、代表性路径或已接受边界时，才进入正文。小功能
可以作为例子，但不作为功能清单。

## 章节与模块边界

前言与第一章分工明确。前言负责人味、背景、致谢和读者契约；第一章负责
技术地图和设计原则。

设备驱动模型应有独立或半独立章节，不埋在 VFS 章里。Anemone 的设备驱动
模型不从属于 VFS：VFS 负责命名、打开与文件对象生命周期；设备子系统负责
设备身份、发布、I/O 语义和私有控制面。`devfs` 是两者之间的桥，它把设备
以路径和 inode 的形式暴露给用户态，但不把设备行为的 owner 转移给文件系统。

pseudo filesystem 是 namespace bridge 和 control surface：它们把内核对象
暴露为路径、文件和目录，也可以承载部分控制操作；但被暴露对象的语义与状态
仍由原 subsystem 拥有。桥接层负责权限、路径、格式化和分发，不反向成为
核心状态的 owner。VFS/namespace 章讲通用原则，设备章专门展开 devfs 作为
device model bridge 的设计。

内存管理章突出折中路线：Anemone 不是纯 Zircon VMO，也不是 Linux VM 的
结构复刻。它在用户可见语义上优先兼容 Linux ABI，在内部组织上吸收 memory
object / backing object 的边界思想，用 Rust 类型、owner boundary 和阶段化
实现来保持系统可维护。

Rust 不单独成章，但第一章应有 `Rust as a Design Constraint` 小节。Rust
不只是实现语言，它影响 API 形状、状态所有权、unsafe 边界和模块可见性；
但 Rust 不自动解决内核问题。书稿应在具体子系统中说明 Anemone 如何压缩
unsafe 和跨 owner 状态流动到可审查边界。

arch / platform boundary 独立成章，覆盖 trap entry/return、syscall dispatch、
interrupt/exception、FPU/context、user/kernel boundary、unsafe assembly 与
Rust ABI 边界，以及 riscv64 / loongarch64 的共性和差异。

IPC / signal / SysV shm 等能力按设计归属分散讲，不按 POSIX 分类机械成章。
signal 归入 task / wait / trap-return delivery；SysV shm 归入 memory object；
pipe、eventfd、timerfd 等只有在能反映 wait-core、I/O object 或 ABI 分层时
才作为例子出现。

## Linux、Zircon 与外部来源

Anemone 兼容 Linux ABI，是为了运行现有用户态程序、测试工具和系统软件；
但这不意味着内核内部结构复刻 Linux。Linux 是重要 ABI 与工程参照，而不是
内部源码形状的目标。

Anemone 可以吸收其他内核和系统的设计优点，例如 Zircon 的 VMO 思路、
对象模型和 capability 风格。书稿应在前言或相关章节中诚实说明设计来源与
致谢，但不能用“像某系统”替代 Anemone 自身的设计解释。

## 代表路径、图与代码片段

每个核心章优先选择一个代表性路径 / case study，而不是罗列全部功能。例如：
syscall adapter、wait/wake、path lookup + mount view、devfs publish +
FileOps/ioctl、file-backed mmap fault、trap entry/return。

图必须有论点。图题可以稍长，允许写成明确技术判断；如果只能写成“某某结构图 /
流程图”，就不应放入正文。第一章可以多一些图，软上限约 4 张；后续模块章
软上限约 3 张。图不要求占满，宁缺毋滥。图题应类似：

- Linux ABI 与 native contract 共享同一组内核对象。
- FdTable、File、Inode 构成 VFS 的三层对象模型。
- Sleep / Wakeup 的关键不变量是先入队再让出 CPU。
- Page Fault 路径把异常处理、地址空间和物理页分配连接起来。

避免只有对象名的图题，例如“VFS 流程图”“内存管理图”“调度器结构图”。

代码片段少量使用，只展示类型 / API 形状或不变量表达，不放长函数实现。
适合展示核心 type、trait、handle、ctx、syscall adapter 与内部 API 差异、
`FileOps` / wait-core / device hook 等抽象边界。

## 验证与限制边界

不单开“工程闭环”章节。全书保持中高层设计文档形态；测试、真实负载、
source audit 等验证事实只有在支撑某个子系统的设计取舍、accepted limitation
或版本边界时自然出现。正文不为展示开发过程而刻意铺陈 feedback loop。

RFC 工作流、transaction devlog、register、write set、review、validation
floor 和 agentic coding 约束等工程方法集中放在附录 C。正文只有在必要时用
脚注或短句指向这些事实层，不反向把开发过程写成叙事主线。

避免写法：

- 为了初赛得分而优先某项能力。
- LTP 是分数占比最大的测例。
- 某组 TPASS 提升了多少。
- 比赛环境要求某个实现形状。

每章不固定写“当前限制 / 未来工作”模板小节，但必须诚实交代边界。限制应
在 trade-off 或章末自然出现，只挑与本章核心设计相关的 accepted limitations，
不搬运 register。

少数章节可以在 `TradeOff: ...` 收束段中讨论 Anemone 当前设计之外的行业参照、
未来演进或更激进的系统设计。这类内容不是 backlog，不承诺实现，也不替代
current limitations；只有当它能帮助读者理解当前 trade-off 的上限或另一种
设计空间时才使用。

## Agentic Coding 与工程工作流

附录 C 标题为“Agentic Coding 与工程工作流”，篇幅中等，不抢占内核设计主线。

重点不是“使用了 AI”，而是 RFC、transaction devlog、register、review 分级、
write set 和 validation floor 如何约束 agent 输出。agent 不能替代设计责任；
所有 accepted contract 仍应回到文档、代码和验证证据。

## 语言与语气

正文使用中文，保留必要英文技术术语，例如 ABI、syscall、wait-core、page
cache、VFS、owner boundary。

文风采用有技术文化感的工程设计报告：主体保持准确、克制、可审查，在章节
开头、脚注和少量过渡语中允许极客文化、社区语境、轻量幽默或相关引语，
让文本读起来像由工程师写给工程师，而不是模板化汇报材料。

边界：

- 每章可以有 epigraph，但必须服务主题。
- 引语如果是名人原话，后续需要核对出处；出处不稳时改为转述或不用。
- 梗可以出现，但不能妨碍评委理解正文。
- 核心论证仍以设计、取舍、不变量、边界和证据为主。
- 不写“业界领先”“完美兼容”等宣传式句子。
