# ANE-CHG-20260731-net-host-test-organization

**Type:** Cleanup
**Status:** Completed
**Date:** 2026-07-31
**Authors:** doruche, Codex
**Area:** network / host validation / repository test entry

## Problem

`net-frame-path`与`net-udp`先后建立host侧验证后，测试内容已经覆盖frame provider、bounded progress、
multi-instance、multi-interface与UDP topology，但仓库没有一个公开入口固定完整矩阵。开发者必须记住具体crate、
feature与no-default编译命令；与此同时，xtask测试仍使用独立的顶层`just xtask-test`，无法与后续Unix/TCP socket
可能增加的repository-owned suite形成一致入口。

stack package内的测试组织也随覆盖增长失去稳定角色：共享fixture全部位于一个近700行的`support/mod.rs`，
12项UDP测试及其fixture全部位于一个近1400行的`udp_topology.rs`。继续直接追加Unix/TCP测试会扩大同一文件中的
协议、fixture与case职责，但当前只有该package的integration tests消费这些能力，不足以建立新的crate或production
surface。

## Scope

本次只增加Justfile拥有的repository test dispatcher，并整理`anemone-smoltcp-stack`既有host test源码布局。
保留五个显式Cargo integration target、各自名称、`required-features = ["host-test"]`、全部case与断言，以及现有
`host-test`/no-default隔离边界。

本次不修改xtask实现，不让xtask拥有或硬编码network crate，不新增test-support crate、依赖、production API、
validation probe、kernel行为或用户可见socket语义。Unix/TCP socket测试及其fixture复用边界留给真实consumer出现后
决定，不在本轮预建抽象。

## Solution

Justfile提供`just test <suite>`，只接受显式登记的`xtask`与`net-host`。`just test xtask`替代独立顶层
`just xtask-test`；`just test net-host`按顺序运行net-api/stack完整host测试、stack no-default test compile和
no-default check。未知suite直接以exit 2拒绝，输入不会被当作命令执行。具体package矩阵留在Justfile私有recipe，
不进入xtask构建系统。

共享测试能力按`clock`、`frame`与`packet`分为owner-local conditional support modules，consumer显式导入所需
能力，不从module root取得宽re-export。UDP composition target保留`udp_topology`身份，但显式Cargo path改为
`tests/udp_topology/mod.rs`；fixture与endpoint、routing、delivery、admission case按职责成为其child modules。
这只是同一test owner内的行为保持拆分，不建立新的编译、可见性或生命周期边界。

本次`Contract Impact: None`：production owner、handoff、failure、cleanup、ABI、effective contract与acceptance
boundary均不变，因此不更新current contract或register，也不升级RFC。

## Change

- 新增公开`just test {xtask,net-host}`，移除独立顶层`xtask-test` recipe；私有recipe继续直接调用各自Cargo命令。
- 将stack test support拆为`support/{clock,frame,packet}.rs`，`support/mod.rs`只声明条件测试模块。
- 将UDP topology target根移动到`udp_topology/mod.rs`，并把原12项测试拆入四个case module及一个fixture module。
- 保留其余四个integration target文件，只把共享fixture import收窄到实际能力模块。
- 既有RFC、transaction与devlog中的`just xtask-test`保留为历史执行证据，不批量改写为当前入口。

## Validation

- `just test xtask`通过，62/62项xtask测试成功。
- `just test net-host`通过：net-api与stack unit分别2/2，bounded progress 7/7，frame path 9/9，
  multi-instance 2/2，multi-interface 2/2，UDP topology 12/12，net-api compile-fail doctest 2/2；stack的
  no-default test compile与check均成功。
- `just --fmt --check`、`just --list`及两个suite的`just --dry-run`通过；公开列表只展示`test suite`，私有
  implementation recipe不外露。未知suite以exit 2失败且不会执行输入。
- `just fmt kernel --check`通过。结构扫描确认恰好五个显式integration target，每个仍由`host-test`保护，UDP目录
  恰好保留12个`#[test]`；xtask与production source均无diff。host-validation facade的15个conditional `Stack`
  入口逐项至少有一个真实integration-test consumer。
- `git diff --check`通过，十个新文件逐项执行`git diff --no-index --check /dev/null -- <file>`均无whitespace
  诊断；`mdbook build docs`通过，只保留既有large search-index warning。
- 未运行QEMU、KUnit、LTP或hardware；本次不修改production代码，也不从host测试外推guest runtime证据。

## Risk / Follow-up

`net-host`目前是network owner的一组精确Cargo命令，而不是自动发现所有host test的框架。Unix/TCP实现若继续位于
同一stack package，应由真实case扩展既有target或增加有明确语义的integration target；只有出现独立编译边界或
外部consumer时才重新评估共享test-support crate。新增suite也必须在Justfile显式登记并给出完整验证范围，不能把
任意参数透传成shell命令。

## Links

- Biweekly devlog: [2026-07-20至2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: 无变化
- Register / limitations: 无新增条目
- RFC / transaction: [Network Frame Path R0](../../rfcs/net-frame-path/index.md),
  [Network UDP R0](../../rfcs/net-udp/index.md)
- 外部源码证据：无
- Issue / PR / commit: 本次聚焦commit
