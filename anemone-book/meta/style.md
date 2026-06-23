# The Anemone Book 风格指南

本文定义 `The Anemone Book` 的文字、编号、图表、代码片段和版式风格。它只
约束书稿表达，不改变 Anemone 的设计事实；设计事实仍以代码、RFC、devlog、
register / current limitations 和外部原始来源为准。

## 总体气质

关键词：克制、密度高、工程感、少装饰、技术文化感、可审查。

目标不是课程报告、API 文档、企业白皮书或博客合集，而是写给系统程序员和
评审者的 design notes：标题有判断，排版有秩序，术语有边界，细节有文化。

正文可以有一点极客风，但酷感来自克制，不来自兴奋。少用感叹号，少用口语化
拟人，不写“是不是很神奇”“大功告成”这类教程语气。

## 编号与交叉引用

正文编号使用 `§`：

```text
§0 Preface
§1 Design Philosophy and System Map
§2 ABI Boundary and Syscall Layer
```

附录使用字母编号：

```text
§A Glossary
§B References and acknowledgements
```

规则：

- 前言从 `§0` 开始；不要写“第零章”。
- 正文引用使用 `§3.2`、`见 §5.1` 这类形式。
- `§` 只用于章节编号和交叉引用，不到处装饰。
- 目录最多三级；不要产生 `§5.1.1.1` 这种深层编号。

## 标题

标题应准确、严肃，并形成稳定导航。它可以是对象型标题，也可以是论点型标题；
不强制每个标题都写成一句判断。对象型标题适合需要完整覆盖重要模块的章节，
论点型标题适合危险路径、关键 trade-off 或收束性小节。

可接受的对象型标题：

```text
§3 Tasks, Processes, and Execution Context
§5 VFS, Namespace, and Pseudo Filesystems
§8 Architecture, Traps, and Platform Boundaries
```

可接受的论点型标题：

```text
§4 Scheduling Observes State, Not Intention
§5 Files Are Objects Before They Are Descriptors
§7 Memory Is a Protocol over Backing Objects
```

较弱的方向：

```text
§4 调度器结构
§5 文件系统
§7 内存管理
```

正文标题中文为主，可保留必要英文关键词。英文标题和英文短语使用 Title Case：
主要单词首字母大写，短介词、冠词、连词按常见英文标题规则小写。后续模板
实现时固定一种样式，不在各章自由发挥。

不要使用“第一章”“第二章”“本章小结”“注意事项”“小贴士”等课程报告式表达。
章末若需要收束，优先使用：

```text
What this buys us
What this costs us
Loose ends
Beyond Anemone
```

这些栏目不是每章必需；只有确实能推动论证时才使用。

## Thesis Paragraph

每个正文章开头应有 thesis paragraph。它不是摘要，而是本章立场。

要求：

- 说明本章希望读者相信什么。
- 点出本章的关键 trade-off 或 owner boundary。
- 不写成“本章首先介绍……然后介绍……”的教材式导语。

示例形状：

```text
调度器常被描述为“选择下一个 task”的机制。这个说法正确，但不够有用。
在 Anemone 中，调度更适合被理解为 runnable state 的所有权边界：谁可以
进入 run queue，谁可以被唤醒，以及哪些状态绝不能被 wait path 偷偷缓存。
```

## 术语

核心技术术语可以保留英文。第一次出现时给出中英对照或本书定义，之后按工程
语境混用。

示例：

```text
运行队列（run queue）
等待队列（wait queue）
文件描述符（file descriptor, fd）
能力式句柄（capability-like handle）
```

规则：

- 核心术语可保留英文，普通动词和连接词用中文。
- 类型、函数、字段、常量、路径使用 monospace，例如 `Task`、`FileOps`、
  `wait_core`、`O_NONBLOCK`。
- 不要把普通概念全包成 monospace。
- 可以在正文中设置“Terminology / 术语约定”短块，说明本书如何使用某个词。

示例：

```text
本文中，task 指可被调度的实体；process 指用户可见的资源容器；thread 指与
另一个 task 共享地址空间的 task。
```

## Callout

使用固定、克制的 callout，不做五颜六色的提示框系统。

推荐组件名使用英文。这些词本身属于工程文化和 specification 语境，翻译后会
损失辨识度。

推荐组件：

- `Invariant`：不变量。
- `Rationale`：为什么这样设计。
- `Trade-off`：收益和代价。
- `Non-goal`：明确不做什么。
- `Boundary`：当前模型或实现的边界。
- `Design Note`：补充说明。
- `Historical Note`：历史或外部系统背景。
- `Footgun`：容易踩错的危险用法，少量使用。

规则：

- 一页不要堆多个 callout。
- callout 必须服务论证，不替代正文。
- 不固定要求每章都有 callout；机械使用会削弱风格。
- `Rationale` 和 `Trade-off` 优先用于关键设计点。

示例：

```text
Rationale

这个设计让 scheduler 不依赖 Linux compatibility layer。代价是，Linux 特有
语义必须在 ABI 边界附近被重新构造出来。
```

## 图、表与 Listing

图必须有论点。如果一张图的标题不能写成明确判断，而只能写成“某某结构图 /
流程图”，就不应放入正文。

编号风格：

```text
Fig. 5.2
Table 3.1
Listing 4.1
```

不要使用“图一”“表一”“代码一”。

图题示例：

```text
Fig. 4.1 — wait-core 拥有阻塞协议，而不是 task。
Fig. 5.2 — FdTable、File 与 Inode 属于三层不同的 ownership。
Fig. 7.1 — Page Fault 连接 trap handling、address space 和物理页分配。
```

适合的图：

- 对象关系图。
- 状态机图。
- 所有权图。
- 层次边界图。
- data path / control path 分离图。
- fast path / slow path 分叉图。
- 生命周期图。
- 锁序图。
- ABI 边界图。

规则：

- 少图，但每张图解释一个结构性事实。
- 不为了装饰放图。
- 不大量使用浅层流程图。
- 图下正文要解释这张图支持了哪个设计论点。
- 图题编号可以使用 `Fig.` / `Table` / `Listing`，但图例解释和正文解读使用中文。

draw.io 图的视觉风格应保持克制：

- 白底或接近白底，少用大面积饱和色。
- 颜色表达角色或层次，不表达装饰。
- 箭头尽量正交，避免斜线穿过节点。
- 分组边框要轻，不做厚重容器墙。
- 文本短，节点名优先使用对象、owner、path 或 invariant。
- 同一章的图尽量使用一致的字号、线宽、圆角和配色。
- 不使用卡通化、手绘化或强烈渐变风格。

draw.io 适合承载需要人工布局的结构图；特别小的示意图、排版符号和局部框图可以
直接用 Typst。无论使用哪种工具，图的最终判断标准都是：读者能从图题和图本身
学到一个结构性事实。

## 代码片段

中高层设计文档不堆代码。代码片段只在能作为证据时出现。

适合三类：

1. 展示不变量的形状。
2. 展示接口边界。
3. 展示短伪代码或状态转换。

规则：

- 使用 `Listing x.y`，不叫“代码块”。
- 不放长函数实现。
- 代码片段周围必须解释“为什么这段值得出现”。
- 不展示宏展开后的长代码。

示例：

```rust
fn wake(task: &TaskRef, reason: WakeReason) -> WakeResult;
```

这类片段的价值在于表达窄接口，而不是展示完整实现。

## 边界、限制与负空间

没有做什么和做了什么同样重要。正文应把限制写成可审查的边界，而不是泛泛的
“缺点”或“不足”。

推荐词：

```text
Boundary
Constraint
Trade-off
Cost
Failure mode
Non-goal
```

规则：

- 主动设计边界写成 `Boundary` / `Non-goal`。
- 暂未补齐但已接受的能力缺口，应自然链接到 current limitations 的事实层。
- 本书不复制 register / current limitations 全文。
- `Beyond Anemone` 只在能帮助读者理解另一种设计空间时使用，不写成 backlog。

## 页眉、页脚与页面

版式偏技术书，不偏论文报告。

建议：

- 正文使用 serif；代码使用 monospace。
- 标题可使用 sans 或 semi-bold serif，后续试排决定。
- 中文字体优先可读性，不为了风格牺牲混排稳定性。
- 页眉可显示 `The Anemone Book` 与当前 `§` 标题。
- 页脚显示页码即可，例如 `37` 或 `p.37`，不写“第 37 页”。
- 装饰线保持细、灰、克制。
- A4 页面留出较宽边距，正文 10.5pt 到 11pt，代码 9pt 到 9.5pt 可作为初始试排范围。

字体候选见 `workflow.md` 的 Typst 项目结构和后续模板实现；最终以本地环境可
稳定渲染为准。

## 版本与封面

主标题固定为 `The Anemone Book`。封面或扉页可以保留一点版本仪式感，例如：

```text
The Anemone Book
Revision 0.1
Built from commit <short-hash>
June 2026
```

可以少量使用技术文化表达，但不要让封面变成玩梗页。

## 语气与脚注

正文保持严肃、准确、可审查。脚注可以容纳少量技术文化或轻微幽默。

规则：

- 梗优先放脚注，不放核心论证。
- 不让梗影响评委理解。
- 不用宣传式形容词，例如“完美兼容”“业界领先”。
- 不用模板味很重的过渡句。

示例语气：

```text
这里没有魔法。真正重要的是边界条件。
```

这类句子可以少量使用；不要连续使用，避免变成姿态。

## 禁用或慎用表达

禁用：

- “第一章”“第二章”。
- “图一”“表一”“代码块一”。
- “本章小结”。
- “小贴士”“友情提示”“知识点”。
- “完美兼容”“全面支持”“业界领先”。
- 未核对来源的名人原话。

慎用：

- 过多英文夹杂。
- 过多 callout。
- 过多脚注梗。
- 只为了酷而写的标题。
- 没有论点的图。

## 应吸收进模板的组件

后续 Typst 模板至少应支持：

- `epigraph`：章首引语。
- `principle` 或 `invariant`：设计原则 / 不变量。
- `rationale`：设计理由。
- `tradeoff`：取舍。
- `boundary` / `non-goal`：边界和非目标。
- `design-note`：克制说明。
- `historical-note`：外部系统或历史背景。
- `book-figure`：带判断式标题的图。
- `book-listing`：带编号和标题的代码清单。

组件数量可以先少后多；模板要服务书稿，不做通用 Typst package。
