# 外部源码引用

Anemone 使用外部内核、libc 和其它系统项目作为 ABI、可见行为与实现比较证据。外部源码是证据，不能自动
覆盖 Anemone 的 current contract、accepted RFC target 或 live implementation。

## 公共参考与私人参考

仓库根目录的 `xref/sources.toml` 是公共参考源注册表。它只收录少量、长期反复使用且具有经典参考价值的
项目，并以 immutable ID、canonical origin 和 full commit 固定内容。实际 checkout 位于 `xref/<id>`，由
`just xref fetch` 物化并被 Git 忽略；checkout 不进入 Anemone 提交，也不是普通 build/test 的网络依赖。

仓库外可能存在更宽松的私人参考或候选 checkout；其位置和组织属于个人环境，不是仓库接口。公共 RFC、
small-change、transaction、contract、register 和 devlog 不得引用私人路径，也不得对某个未固定版本的私人
checkout 作事实声明。若一份私人参考开始成为反复使用的公共依据，应通过普通 review 把精确 origin/commit
加入 `xref/sources.toml`；若只服务一次调查，可以直接使用固定 commit 的普通 upstream permalink，而不必因此
扩张公共 registry。

## 规范引用形式

公共注册表中的源码使用以下规范标签：

```text
xref:<source-id>:<repo-relative-path>[#<locator>]
```

- `<source-id>` 必须与 `xref/sources.toml` 中的 immutable `id` 完全一致；不重复书写 commit。
- `<repo-relative-path>` 从上游仓库根开始，不写任何本机 checkout 路径或绝对路径。
- `<locator>` 省略时表示整份文件；引用具体实现结论时应尽量提供。
- symbol locator 使用源码中的稳定名称，例如函数、类型、常量或 `Type::method`。
- 精确布局、常量表或短 ordering 片段可以使用一基、闭区间的 `L<start>-L<end>`；单行写 `L<line>`。
- 优先用 symbol 表达语义位置；只有结论依赖精确文本范围，或源码缺少稳定 symbol 时才用行号。

例如：

```text
xref:linux-6.6.32:drivers/tty/n_tty.c#n_tty_read
xref:xv6-riscv-20260717:kernel/proc.c#scheduler
xref:xv6-riscv-20260717:kernel/proc.c#L425-L463
```

引用必须放在它所支持的具体 claim 附近。一个 source label 只能证明对应 commit 中的源码事实，不能把上游
设计选择写成 Anemone 已接受的目标、内部 owner 或用户 ABI。

## Markdown 永久链接

当上游提供稳定代码浏览器时，推荐把 canonical label 作为 Markdown link text，并让 URL 固定到 manifest 中
的同一个 full commit：

```md
[`xref:xv6-riscv-20260717:kernel/proc.c#scheduler`](https://github.com/mit-pdos/xv6-riscv/blob/b6dd660d4903947e5eb75ae9a457854f3707eb14/kernel/proc.c#L425-L463)
```

URL 不能使用 branch、`HEAD`、默认分支或仅靠可移动 tag。浏览器 URL 的行锚点只服务点击定位；canonical
label 中的 symbol/line locator 才是仓库文档使用的引用身份。若上游没有稳定浏览器，保留 code-form label
即可，读者可以运行 `just xref fetch <source-id>` 后在固定 checkout 中核对。

## 文档使用规则

- RFC 和 small-change 应在具体源码判断旁内联 citation；集中证据较长时放入对应 `backgrounds/`，正文链接
  该证据页，不复制大段外部源码。
- transaction 和 devlog 只在执行事实确实依赖外部源码判断时引用；不要把同一引用复制成第二份设计权威。
- current contract 可以链接证明 cutover 的 RFC/transaction evidence，但不靠外部源码 citation 代替
  Anemone 自身的 effective rule。
- 已有历史文档不批量迁移。后续第一次修改相关证据或重新依赖该判断时，再换成 canonical citation。
- 复制外部代码仍必须独立检查上游许可证；公共 registry 身份不等于复制授权。
