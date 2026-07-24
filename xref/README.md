# 外部源码参考

本目录保存仓库精选的外部源码元数据。源码 checkout 是 `xref/<id>` 下被 Git 忽略的本地物化结果；它们不是
Anemone 源码、构建输入、当前契约或 RFC 目标。

`sources.toml` 是规范注册表。每个条目包含：

- 不可变的 `id`，同时也是 checkout 目录名；
- 英文 `scope`，说明该源码适合参考什么，以及不具有什么权威；
- 规范的只读 HTTPS Git `url`；
- 可选的上游发布 `tag`；
- 标识所选内容的完整 `commit`。

公共文档引用某个 ID 后，不得把该 ID 改指向另一份内容；不同源码快照必须新增条目。tag 只提供获取和来源
追溯提示，commit 才是内容身份；xref 会验证 tag peel 后是否等于注册的 commit。

通过仓库入口查看或物化参考源码：

```text
just xref list
just xref fetch linux-6.6.32
just xref fetch --all
just xref check linux-6.6.32
just xref check --all
```

`fetch` 会将源码 clone 到 `xref/<id>` 并 checkout 为 detached HEAD，不初始化上游 submodule。已有且匹配的
clean checkout 会幂等成功；已有 non-Git 目录、origin 或 commit 不符、以及 dirty checkout 都只报告错误，不会
被修改。普通构建、测试和文档流程不会 fetch 或依赖这些源码。

公共证据使用[外部源码引用规则](../docs/src/external-source-references.md)定义的规范形式：
`xref:<source-id>:<repo-relative-path>#<locator>`。不得引用私人 checkout 路径。能够读取参考源码也不等于获准
把上游代码复制进 Anemone；复制前仍须审查上游许可证和本仓库的许可证边界。
