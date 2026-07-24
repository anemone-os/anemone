# ANE-CHG-20260724-xref-source-registry - xref 外部源码注册表

**类型：** 工具链
**状态：** 已完成
**日期：** 2026-07-24
**作者：** doruche、Codex
**领域：** 开发工具 / 外部参考 / 文档

## 问题

用于 ABI 和实现研究的外部内核、库此前通常只存在于私人 checkout。公共 RFC 证据虽然可以写 release 和源码
路径，仓库却没有规范 origin、不可变 commit、权威边界或安全的物化命令。Git submodule 能固定内容，但也会让
这些可选研究资料看起来像 Anemone 源码或构建依赖。

## 范围

本轮增加 tracked Git 源码注册表，以及由仓库拥有的 list、fetch、check 命令，并登记 Linux 6.6.32 和当前的
MIT PDOS xv6-riscv 快照。物化仓库仍是 `xref/<id>` 下被忽略的本地数据，不是 build、test、RFC target 或
current contract 的输入。

本轮不 vendor 外部源码，不添加 submodule，不初始化上游 submodule，不批量改写历史 RFC 引用，也不让普通
build 和 validation 流程访问网络。

## 方案

`xref/sources.toml` schema 1 只保留已有 consumer 使用的字段：不可变 `id`、英文 `scope`、规范 HTTPS Git
`url`、可选 release `tag` 和完整 40 字符 `commit`。不增加结构化 `roles`、可移动 branch、checkout path、
timestamp 或 fetch policy。`xref list` 展示 `scope`；`id` 同时定义固定 checkout 路径，因此 CLI 不需要 root
选项。

Justfile 把 `just xref` 路由到现有 xtask owner。`fetch` 先在同目录临时 checkout 中 clone，有 release tag 时
验证 tag，随后以 detached HEAD checkout 已登记 commit，核对 origin、HEAD 与 clean worktree，最后发布为
`xref/<id>`。已有且匹配的 checkout 幂等成功；non-Git、origin 错误、commit 错误或 dirty target 都会在不修改
目标的情况下被拒绝。失败的临时 checkout 也进入命令 cleanup。`check` 复用相同的身份与 clean 检查，但不访问
网络。

`xref/.gitignore` 忽略所有物化或临时 checkout，只显式放行注册表元数据和 ignore policy。这使外部仓库不会
进入 Anemone Git index，同时也不把其生命周期交给 `clean`。

公共 RFC 和小迭代使用 `xref:<source-id>:<repo-relative-path>#<symbol-or-lines>` 引用外部源码。source ID 通过
注册表提供不可变 commit，路径从上游仓库根开始；语义判断优先使用 symbol locator，精确布局或短 ordering 证据
仍可使用一基行号范围。固定到相同 commit 的上游 permalink 可以让引用可点击，但私人路径、branch 和 `HEAD`
都不能成为引用身份。

## 变更

- 新增 `xref/{README.md,sources.toml,.gitignore}`，定义注册表、引用、不可变性、fetch 与许可证边界。
- 登记 Linux stable `v6.6.32`，tag peel 后的 commit 为
  `91de249b6804473d49984030836381c3b9b3cfb0`。
- 登记 MIT PDOS xv6-riscv commit `b6dd660d4903947e5eb75ae9a457854f3707eb14`；其上游 commit 日期为
  2026-07-17，ID 为 `xv6-riscv-20260717`。
- 新增 `just xref {list,fetch,check}` 以及 xtask manifest/fetch/check 实现，并用本地 Git fixture 覆盖。
- 在 `xref/<id>` 下物化并检查两份注册源码；checkout 被 Git 忽略并保持开发者本地状态。
- 新增公共外部源码引用规则，并让 RFC 工作流、RFC 模板与小迭代模板使用同一规范格式；不批量重写历史证据。

## 验证

- 官方 remote lookup 将 Linux `v6.6.32^{commit}` 解析为
  `91de249b6804473d49984030836381c3b9b3cfb0`，将 MIT PDOS xv6-riscv HEAD 解析为
  `b6dd660d4903947e5eb75ae9a457854f3707eb14`；GitHub commit 记录日期为 2026-07-17。
- `just xtask-test` 69/69 通过，包括 manifest 校验，以及覆盖首次 fetch、幂等 re-fetch、clean check 和拒绝
  dirty checkout 的本地带 tag Git fixture。
- `just xref --help` 与 `just xref list` 展示固定根目录接口和两个英文 scope；xtask CLI 测试会拒绝已删除的
  `--root` 选项。
- `just xref fetch xv6-riscv-20260717`、`just xref check xv6-riscv-20260717` 和重复 fetch 通过；checkout 的
  origin、完整 HEAD 和 clean 状态均与 manifest 一致。
- 用户物化 Linux 后，`just xref check --all` 通过；Linux origin、完整 HEAD 和 clean 状态均与 manifest 一致。
- `git check-ignore -v` 将两份已注册 checkout 和一个代表性临时 checkout 路径映射到 `xref/.gitignore`；主仓库
  `git status` 不会枚举下载的源码文件。
- `just fmt all --check`、`mdbook build docs` 和 `git diff --check` 通过；每个 untracked 新文件的独立 no-index
  whitespace check 均无诊断。

## 跟踪问题

无。

## 风险与后续

- 没有 tag 的 Git 源会先执行普通 no-checkout clone，再选择不可变 commit。xv6 仓库较小，因此接受这一行为；
  未来若登记大型且无 tag 的源码，应依据实测结果优化 fetch，而不是预先增加 manifest 字段。
- `check --all` 有意要求所有已登记源码都已在本地物化。只需要一份源码的开发者仍可按 ID 单独 fetch 和 check。

## 链接

- 双周开发日志：[2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- 当前契约：无
- 登记册 / 已知限制：无
- RFC / 事务：无
- 外部源码证据：无；本轮定义引用机制，没有从外部代码推导 Anemone 行为结论。
- 问题单 / PR / commit：当前工作区 diff，commit 待创建
