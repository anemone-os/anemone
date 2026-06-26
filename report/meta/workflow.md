# 报告写作流程

## 入口顺序

写某个章节前，按顺序阅读：

1. `report/requirements.md`
2. `report/meta/style.md`
3. 本文件
4. `report/meta/outline.md`
5. `report/meta/sources.md`
6. 目标 `report/content/*.typ` 文件

如果任务涉及 Typst 版式，还要阅读 `report/main.typ`、`report/conf.typ` 和
`report/components/*.typ`。

## 证据门槛

每个技术小节在写正文前，至少需要一个证据入口。证据入口可以是：

- 代码路径、类型、函数、模块或 syscall 实现。
- 测例、本地测试脚本、LTP case、profile 或端到端日志。
- RFC、transaction devlog、小迭代 devlog、register 记录或 current limitation。
- 明确的问题、失败现象、死锁、竞态、errno 不匹配、panic 或性能问题，并且有足够
  上下文可以核对。

如果缺少证据：

- 不得编造事实。
- 不得用泛泛而谈的文字填充小节。
- 留下 TODO 或简短章节备注，说明缺少什么来源。

## 草稿形状

模块小节优先按这个顺序写：

1. 这个模块负责什么。
2. 主要数据结构或路径是什么。
3. 一个具体机制或代表路径。
4. 有证据时，写一个代表性问题或取舍。
5. 当前限制或验证状态。

工程过程和 AI 小节优先按这个顺序写：

1. 规则或原则。
2. 具体例子。
3. 边界或失败模式。
4. 验证或纠正方法。

## 与其他材料的关系

`anemone-book/` 是材料来源，不是报告风格来源。只能在核对代码、RFC、devlog、
register 或 current limitations 之后，复用其中的事实、模块地图和候选例子。

`report/refs/*.pdf` 是风格和结构参考。不得复制其他项目的具体说法、图或结论。

`report/requirements.md` 是评审要求来源。如果风格选择和评审要求冲突，以评审
要求为准。

## 审查清单

完成一段报告正文前，检查：

- 每个技术小节是否有证据入口？
- 主语是否是具体 OS 机制，而不是抽象口号？
- 完成情况、性能、排名、测试结果是否可验证？
- 支持不完整时，是否写清限制？
- AI 使用说明是否包含工具、范围、人工审查和验证？
- 这段话是否像队员能在答辩中解释？

## 验证

修改 Typst 或正文后运行：

```sh
make -C report
git diff --check -- report
```

新增未跟踪文本文件时运行：

```sh
git diff --no-index --check -- /dev/null <file>
```

大幅修改后，用 `pdftotext` 检查摘要、目录、表格和明显版式回归。
