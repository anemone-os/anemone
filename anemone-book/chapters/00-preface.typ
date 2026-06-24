#import "../template/components.typ": *

= 前言

#epigraph(attribution: [Kent Beck])[
  Make it work, make it right, make it fast.
]

#thesis[
  这只是 `§0` 的 seed。它先固定读者契约和叙述边界，不急着写成最终开场。最终版前言要等模块章节完成主要 draft / review 后再回写。
]

Anemone 不是 Linux 的 Rust 复刻，也不是一组 syscall patch 的合集。本书试图回答的问题更窄：一个面向 Linux ABI compatibility 的教学/竞赛内核，如何在 Rust 的类型约束、Anemone 自己的对象模型和持续验证之间，形成一套可以审查的设计叙事。

本书不是手册，也不是事实源。它只把已经存在的设计事实组织成一条可读路径；设计事实仍可由源码、已接受设计文档、执行记录、开放问题和已接受限制核对。#footnote[本书不替代这些 canonical 文档。]

#non-goal[
  本书不以测例分数组织叙事，也不把开发过程描述成比赛环境的逐项应对。验证可以作为工程反馈闭环出现，但它服务设计判断，而不是替代设计判断。
]

后续章节会从系统地图开始，依次展开 ABI 边界、执行实体、调度与等待、VFS、设备模型、内存、架构边界和工程反馈闭环。这个顺序不是实现顺序，而是读者理解系统的路径。
