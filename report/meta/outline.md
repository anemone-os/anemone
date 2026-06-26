# 报告章节职责

本文记录章节职责。它不是进度表。

## 全局写作粒度

每章只需要讲出评委最需要判断的内容：完成了什么能力、关键机制怎么组织、为什么这样
取舍、有什么代表性验证或问题、边界在哪里。不要把常规 OS 机制按源码路径拆成完整
导览；除非细节能体现 Anemone 的区分度、解释复杂 bug、支撑 ABI 边界或评审判断，
否则压缩成机制结论。

未来路线、项目价值和竞赛表达可以出现。不要把它们机械改写成“当前限制”，也不要删到
只剩缺陷清单。agent 不得自行判断某句话是否“宣传”“空泛”或“不可验证”并据此删除；
只有发现明确事实冲突、与用户大纲冲突，或用户明确要求清理时，才改写。
如果 agent 认为某个正向表述可能有竞赛表达风险，但不能证明它事实错误或与大纲冲突，
必须停下来问用户留不留，不能先删后报。

除非本大纲、具体章节安排或用户明确指令要求写限制 / 边界，否则正文不得自行新增
“当前限制”“当前边界”“阶段性边界”等防守性段落。限制写作只能跟随大纲，不由 agent
临场补充。

正文禁止写作元叙述。`如果这里要扩展，未来可以...`、`正式正文需要...`、`本节应当...`
这类句子只能留在 meta 文档、草稿备注或 TODO 中，不能作为报告正文。若其中包含真实
路线，改写成直接的项目展望；若只是写作提醒，删除。

## 前置部分

### `content/00-abstract.typ`

目的：在目录之前，让评委快速知道项目是什么、做到哪里、报告会讲什么。

必须包含：

- 项目身份和目标平台。
- 当前排名或分数截图（有最终截图后替换）。
- 模块完成情况表。
- 简短说明报告会解释实现、验证、团队工作和 AI 使用。

避免：

- 长篇设计哲学。
- 仅因为某句话面向未来，就删掉有依据的开源价值或后续路线。

## 正文章节

正文只讲内核设计层面的内容。测试复现、RFC 工作流、agentic coding 和 AI 使用
不单独作为正文章节，只作为模块论证中的证据，或放入附录。

### 1. 概述

目的：介绍项目目标、整体架构、代码结构、团队贡献和参考来源。

需要证据：

- 仓库目录结构。
- 队员贡献事实。
- 实际参考过的公开项目或文档。

### 2. 进程管理

目的：解释进程 / 线程模型、生命周期、资源继承、拓扑关系、exec / exit / wait
和进程可观测性。

候选证据：

- Task 和 topology 相关类型。
- fork / clone / exec / exit / wait 路径。
- fd table、地址空间、credentials、工作目录和根目录继承边界。
- `/proc/<pid>`、wait 返回值、进程组 / session 相关测例或 devlog。

### 3. 调度

目的：解释 scheduler 状态、runnable queue、wait-core、唤醒协议、可中断等待、
同步原语和多核负载分配。进程生命周期放在第二章，时间线、soft timer 和用户可见
时间对象放在第八章，硬件时钟入口放在第九章。

候选证据：

- 调度队列或 runnable state 代码。
- wait / latch / wake token 路径。
- 与 sleep、timeout、signal interruption、pipe blocking、poll / select 相关的
  LTP 或 libcbench 失败。

### 4. 内存管理

目的：解释地址空间、页表、VMO / backing object、fault handling、COW / lazy
allocation、mapping、共享内存和内存压力路径。内存管理前置是为了让评委较早看到
Anemone 相对往届内核更有区分度的 memory object / VMO 设计。

候选证据：

- address space 和 VMA 类型。
- VMO / backing object 路径。
- page fault handler。
- page allocator。
- SysV shm 和 mmap 实现。
- register / current limitations 中接受的 VM 限制。

### 5. IPC

目的：解释 signal delivery、pipe、System V IPC 和 event-like file object。共享内存
的 IPC API 放在本章，页映射和 fault path 放在第四章。

候选证据：

- signal action、pending、mask、target selection、sigreturn 路径。
- pipe buffer 和 close / drop 唤醒行为。
- 支持时的 SysV shm / msg / sem 路径。
- 支持时的 eventfd / timerfd / signalfd 或 poll / select 集成。

### 6. 文件系统

目的：解释 VFS 对象模型、路径查找、mount view、filesystem backend、procfs 和
devfs。

候选证据：

- `File`、`FileDesc`、`Inode`、`Dentry`、`PathRef`、`Mount` 代码路径。
- mount-tree RFC / devlog。
- procfs task / status / fd / mounts 路径。
- devfs bridge 路径。

### 7. 设备驱动模型

目的：解释 device model、char / block device、devfs bridge 和 ioctl dispatch。

候选证据：

- device registration 和 probe 路径。
- char / block device trait 或 file ops。
- loop / block / random / tty 实现。
- ioctl routing 和代表性 command 处理。

### 8. 时间

目的：解释 clock、tick、timekeeper、soft timer、IRQ / threaded timer lane、
`nanosleep` / timeout、`timerfd` 和 `ITIMER_REAL`。本章说明时间对象和等待协议、
文件对象、信号机制之间的关系；硬件 timer interrupt 的架构入口放在第九章。

候选证据：

- `time` 模块的 clock、timekeeper、instant 和 timer 路径。
- threaded timer event RFC / transaction devlog。
- `timerfd` 小迭代 devlog。
- `nanosleep`、`clock_nanosleep`、`timerfd`、`setitimer` / `getitimer` 对应路径。
- 与 sleep timeout、timerfd、itimer、signal interruption 相关的 LTP 或本地日志。

### 9. 架构硬件抽象层

目的：解释 RISC-V 与 LoongArch 的架构差异，以及通用内核代码如何通过 HAL 保持
共享。启动、Trap、中断、上下文切换、用户态返回和硬件时钟入口都归入本章；clock、
soft timer 和用户可见时间对象放在第八章。

候选证据：

- 两个架构的启动文件。
- trapframe / context 差异。
- timer / interrupt / platform 代码。
- 各架构构建或运行验证。

### 10. ABI 兼容设计

目的：解释 Linux ABI 兼容、syscall dispatch、UAPI 参数解析、flag / errno 语义
和代表性 syscall 路径。本章后置到核心模块之后，避免一开篇进入抽象 ABI 叙事；
写作时应连接前文的 task、VFS、MM、device、wait 和 arch。

候选证据：

- syscall dispatch 和 syscall 注册路径。
- 代表性 syscall，例如 `openat`、`clone`、`mmap`、`ioctl`、`pselect6`。
- Linux / LTP / man-pages 对应语义。
- 相关 current limitations 或 devlog 记录。

### 11. 总结与展望

目的：总结已完成工作、开发经验、当前限制和未来计划。

必须包含：

- 按能力域总结工作。
- 实际调试和工程经验。
- 诚实的当前限制。
- 基于当前缺口的未来工作。

避免：

- 引入前文没有讲过的新技术声明。
- 把总结写成当前限制清单，削弱已完成工作的判断。
- 因为 agent 主观觉得“宣传”而删掉用户大纲里的正向表达。

## 附录

### 附录 A：AI 使用情况

目的：满足 AI 使用披露和工程可追溯要求，但不抢占内核设计正文。

必须包含：

- RFC / transaction devlog / register / current limitations 如何约束实现范围、
  停止条件、验证和回写。
- agentic coding 的 write set、review、validation floor 和人工判断边界。
- AI 工具、使用场景、生成内容范围、人工审查、验证和幻觉纠正案例。

### 附录 B：参考资料

目的：列出实际参考过的公开资料、往届内核、开源项目和使用范围。

避免：

- 把其他项目的项目事实写成 Anemone 自身事实。
- 只列名字，不说明借鉴范围。
