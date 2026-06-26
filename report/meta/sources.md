# 报告事实来源

写报告正文前，用本文确定事实来源。

## 评审要求

- `report/requirements.md`：评审准则、AI 使用披露、通用性、隐藏测试、能力域和
  文档要求的主要来源。

## 风格参考

- `report/refs/Chronix-初赛文档.pdf`
- `report/refs/Del0n1x初赛文档.pdf`
- `report/refs/NighthawkOS初赛文档.pdf`
- `report/refs/Unicus初赛文档.pdf`

这些 PDF 只用于参考结构、密度和开发报告体裁。不得复制其他项目的项目事实、
图、表述或结论。

从参考文档中观察到的报告形态：

- 封面。
- 摘要在目录之前。
- 前部有模块完成情况表和排名图。
- 章节按 OS 能力域组织。
- 使用代码片段和图说明具体机制。
- 开发经验和未来工作放在后部。
- AI 使用可以放入附录，但必须具体、可追责。

## Anemone 公开工程记录

已接受设计、执行证据和已知限制优先从这里取：

- `docs/src/rfcs/`
- `docs/src/devlog/transactions/`
- `docs/src/devlog/changes/`
- `docs/src/register/`
- `docs/src/register/current-limitations.md`

报告正文中使用稳定路径、文档标题、模块名或符号名。避免在最终报告里使用脆弱的
行号引用。

## 代码和本地证据

代码是当前行为的真相源：

- `anemone-kernel/`
- `anemone-apps/init/`
- `anemone-apps/user-test/`
- `scripts/run-user-test-rv64.sh`
- `scripts/run-user-test-la64.sh`

`etc/` 下的本地私人证据可以指导起草和调试，包括 vendored LTP / Linux / testcase
源码。但最终公开正文不应把私人 `etc/` 路径当作稳定公共来源，除非该段明确是在说明
本地开发证据。

## Book 材料

- `anemone-book/`

只把它当作事实、模块地图和候选解释的材料池。报告不得继承 book 面向技术读者的
叙事口吻。从 book 复用的说法必须先用代码、RFC、devlog、register 或 current limitations
核对。

## 资产

- `report/assets/school.jpg`：封面图片。
- `report/assets/rank.png`：当前排名截图占位。

提交前用最终截图替换排名资产。
