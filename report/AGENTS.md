# 开发报告写作协议

本目录是 Anemone 面向比赛评委 / 老师的开发报告。它不是技术读物，不是 RFC
压缩版，也不是宣传页。

除代码标识符、路径、命令、必要英文技术术语外，报告正文和本目录内的报告
流程文档默认使用中文。

修改报告正文前，必须先阅读：

1. `report/requirements.md`
2. `report/meta/style.md`
3. `report/meta/workflow.md`
4. `report/meta/outline.md`
5. `report/meta/sources.md`

然后再阅读目标章节 `report/content/` 下的 `.typ` 文件。

硬约束：

- 写作口吻应像队员在解释自己能答辩追问的工作。
- 每个技术小节必须有具体证据入口：代码路径、测试、日志、RFC/devlog/register
  记录，或明确的问题 / bug。
- 没有证据入口的技术判断不得进入正文，只能留下 TODO 或章节备注。
- 主体章节按 OS 能力域和具体机制组织。`anemone-book` 中的抽象设计词可以用来
  解释局部取舍，但不能成为报告主线。
- 不得编造完成情况、测试结果、排名、队员、指导老师、AI 使用情况或验证证据。
- 不要把私人 `etc/` 路径写成公开稳定来源，除非该段明确是在说明本地开发证据，
  且文字清楚交代了这个边界。

构建与验证：

```sh
make -C report
git diff --check -- report
```

对于新增且未跟踪的文本文件，还要使用 `git diff --no-index --check -- /dev/null
<file>` 检查空白问题。


## 如果当前目录存在LOCAL.md，请阅读。