= Agentic Coding 与工程工作流

出于提交透明性要求，本节先说明 agentic coding / writing 在 Anemone 开发和本书生产中的使用范围；随后更重要的是解释这些输出如何被工程工作流约束。这里讨论的核心不是“使用了 AI”，而是 Anemone 如何把 agent 输出放进一套可审查的 engineering harness 中：目标边界、write set、source pass、review、validation floor 和 canonical docs 共同决定什么可以进入代码、文档和对外叙述。

== 使用范围与责任边界

Anemone 的开发过程中，agent 参与过 source pass、方案比较、草案整理、review checklist、局部代码实现、重构建议和书稿起草。agent 也会帮助把运行结果、review 结论和实现反馈整理到 RFC、transaction devlog、小迭代记录或 register 中。这样的参与不是设计责任的转移：accepted contract、scope、实现取舍、合并判断和最终解释责任仍由维护者承担。

因此，本书不把 prompt、对话记录或 agent 内部推理当作事实来源。进入代码或书稿的判断必须能回到源码、RFC、devlog / transaction devlog、register / current limitations，或明确的验证结果。agent 可以生成候选实现和候选表述，但不能用“agent 这么写的”替代 review，也不能在出错时成为责任出口。

== Engineering harness

这里的 harness 不是测试 runner，而是一组工程约束。它先把任务限定在明确目标和 write set 内：agent 可以在指定文件、章节或阶段内推进；如果发现正确设计需要触碰新的 owner surface、shared contract 或公共事实层，正确动作是停止并上报扩展理由，而不是在原范围内做兼容性绕路。

source pass 负责把候选事实和材料入口固定下来，避免凭记忆补齐实现状态。review 负责检查 owner boundary、ABI boundary、状态所有权、生命周期和失败路径是否仍然成立。validation floor 负责说明当前阶段至少需要什么证据：可能是 `git diff --check`、`typst compile`、`just build`、定向 LTP、source audit，或用户侧运行证据。canonical docs 则负责把设计事实和执行事实分层保存，避免聊天上下文成为隐形真相源。

== Review 等级

Anemone 的 review finding 使用五个等级。等级的作用不是给意见贴标签，而是决定问题在工作流中的处理边界。

#figure(
  table(
    columns: (1.1fr, 2.4fr, 2fr),
    inset: 6pt,
    align: (left, left, left),
    table.header([等级], [含义], [处理边界]),
    [`Apollyon`], [错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态。], [必须修正。],
    [`Keter`], [不会立刻爆炸，但会阻塞后续开发、污染状态所有权或把核心抽象带错方向。], [必须修正，或转成明确实现 gate 的停止条件。],
    [`Euclid`], [通常值得修，但不阻塞主线。], [可以带入实现阶段，但必须有验证点和回写路径。],
    [`Safe`], [记录即可，除非局部且低成本。], [默认不为完美主义阻塞推进。],
    [`Neutralized`], [已经处理完成的问题。], [保留处理依据和对应事实入口。],
  ),
  caption: [Review 等级决定问题是否阻塞、进入 gate，或只作记录。],
)

这个等级体系防止两种相反错误：把所有建议都当 blocker，或者把真正影响 owner boundary 的问题降级成普通 TODO。对 agentic coding 来说，它也是一种停止机制：当问题已经进入 `Apollyon` 或 `Keter`，agent 不能继续用局部实现把风险埋过去。

== 前馈设计与受控反馈

Anemone 的 RFC 工作流区分前馈约束（feedforward constraints）和受控反馈（controlled feedback）。前馈约束是设计期先闭合的 accepted contract、invariant、owner boundary、validation floor 和停止条件；它让 agent 在实现前知道哪些边界不可削弱。受控反馈则承认大型实现中的接口摩擦、状态机细节、错误路径和集成风险常常只有在真实编码或 vertical slice 中才会暴露。#footnote[`docs/src/rfc-workflow.md`; `docs/src/devlog/changes/2026-06-18-rfc-feedback-loop.md`]

这并不等于放弃设计闭合。实现期反馈可以调整阶段顺序、write set、验证方式或局部接口形状；如果反馈触碰目标、不变量、ABI 边界或接受边界，就必须停止当前 gate，回到 RFC review，并更新 canonical 文本。反馈只能优化路线，不能把必须满足的不变量降级成建议项，也不能用临时 hack 接受更弱语义。

== 文档层与事实归属

Anemone 的开发者文档承担不同层次的事实归属。附录 C 只引用这些稳定入口，不引用私人草稿路径、对话日志或临时上下文。

#figure(
  table(
    columns: (1.25fr, 2.2fr, 2.2fr),
    inset: 6pt,
    align: (left, left, left),
    table.header([Artifact], [承担], [不承担]),
    [`RFC`], [accepted contract、invariant、阶段 gate、接受边界], [执行流水账],
    [`implementation.md`], [阶段计划、write set、验证 floor、停止条件], [已执行 checkpoint],
    [`transaction devlog`], [执行事实、review 结论、验证证据、实现反馈], [重新定义 accepted contract],
    [`register / current limitations`], [当前开放问题和接受限制], [设计草案或实现计划],
    [`The Anemone Book`], [设计叙述聚合], [Anemone 的 canonical source],
  ),
  caption: [工程工作流中的事实归属。],
)

这个分层也约束 agent。agent 可以帮助建立或更新这些文档，但不能让某一层承担另一层的职责：transaction devlog 不重新定义 contract，register 不保存设计草案，book 不反向规定内核事实。

== 工作流样本

`Sched Latch` RFC 展示了前馈约束如何先把等待协议收束住。它把 `Latch` 定义为单轮 OR wait 组合器，明确 single-consumer owner boundary、producer trigger capability、source register gate、final readiness scan 和 wait-core stale-safe placement。这样的 RFC 不只是计划文件；它让后续实现和 review 都围绕同一组不变量判断。#footnote[`docs/src/rfcs/sched-latch/`]

`Threaded Timer Event` RFC 展示了受控反馈的边界。它接受第一版 threaded timer lane，用来迁移 `timerfd` 和 `ITIMER_REAL` 的 bounded process-context completion；同时明确不把 threaded timer 扩成通用 workqueue，不迁移 wait-core timeout，不引入物理取消、drain、worker pool 或 periodic timer core。实现期反馈可以补强 gate 和证据，但不能扩大第一版语义。#footnote[`docs/src/rfcs/threaded-timer-event/`]

`user-test-staged-tools` 小迭代展示了较小粒度的 harness。它把缺少 `mkfs.ext4` 工具的问题归类为测试设施缺口，而不是混进 VFS、block、loop 或 mount 语义失败；方案只建立 binary-safe staged tool 通道，不顺手做包管理、依赖解析或版本校验。这种记录让外部信号先被正确分类，再进入对应层次的实现或限制说明。#footnote[`docs/src/devlog/changes/2026-06-09-user-test-staged-tools.md`]

== 失败模式与约束

agentic coding 的主要风险不只是“会写错事实”。更常见、也更难在短期测试中暴露的问题，是工程形状被局部便利带偏。

第一类是过度抽象。agent 容易把一次性逻辑拆成小 wrapper、helper 或 mini framework，看起来更整齐，实际没有减少复杂度，反而让读者寻找不存在的复用意图。Anemone 的约束是简单优先：没有真实复用、复杂度下降或已存在本地模式时，不引入新抽象。

第二类是预铺接口。阶段性实现中提前为下一阶段留 API，表面上像前瞻设计，实际上会误导后续实现者：这个接口是否已经承诺、是否可以删除、是否仍符合后续发现的真实边界，都不清楚。Anemone 更倾向于把下一阶段需求写进 RFC 或 transaction devlog 的 gate，而不是把未验证接口沉进代码。

第三类是强行闭合。agent 为了让功能“做出来”，可能写出别扭路径、隐藏失败、降低验证集合，或用日志和静默兼容替代应有语义。harness 要求在做不到、scope 不对或 contract 不成立时停下来，回到 RFC review、write set 扩展或 limitation 分类，而不是让 hack 变成事实。

第四类是状态所有权漂移。为了局部实现方便，agent 可能把状态判断、缓存字段、诊断字段或 compatibility bridge 放到错误层，形成第二套真相源。Anemone 的 review 会把 single source of truth、diagnostic-only field、narrow interface 和 temporary bridge exit condition 当成实质问题，而不是代码风格问题。

这些约束不会让 agentic coding 变得“自动安全”。它们的作用更有限，也更实际：让 agent 输出必须经过人可以复审的事实层、边界层和验证层。速度来自自动化协助，可靠性仍来自工程约束。
