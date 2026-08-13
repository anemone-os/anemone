# ANE-CHG-20260812-workspace-config-locators

**Type:** Small Feature
**Status:** Completed
**Date:** 2026-08-12
**Authors:** doruche, Codex
**Area:** xtask / build configuration / resolver

## Problem / Context

BuildPreset、SystemTarget与Platform reference过去只能使用严格slug，并分别固定到
`conf/build-presets/`、`conf/system-targets/`与`conf/platforms/`。这适合tracked canonical inventory，
但无法让同一个build/QEMU resolver显式消费workspace内的私有或临时配置图；开发者只能提交一次性配置，
或者在公共目录外另建绕过resolver的入口。

本轮只扩展配置文件定位能力。它不增加local/default selection，不改变explicit preset与完整tuple互斥，
不允许workspace外配置，也不改变Platform、SystemTarget、KernelConfig、BuildPreset或invocation facts的owner。

## Decision

BuildPreset、SystemTarget与Platform共用canonical-first locator语义。没有显式`./`的合法slug先尝试对应
`conf/<kind>/<slug>.toml`；只有该目录项不存在时，才把输入精确解释为workspace-root-relative路径。`./`
强制workspace路径，用于同名canonical文件存在时的显式消歧。nested target/platform locator同样相对
workspace root，不相对referring manifest。

canonical目录项一旦存在就拥有优先权：dangling symlink、目录、越界symlink、read或parse failure均直接失败，
不触发fallback。所有path拒绝绝对路径、词法逃逸、symlink逃逸与非普通文件。resolver把最终选择的相对路径
保留为本action snapshot的诊断provenance；parsed config仍是行为的唯一输入。

这是一个owner-local locator与current contract refinement；owner、handoff、failure cleanup、kernel ABI和
runtime acceptance均已闭合，因此不需要RFC。

## Change

- `BuildPresetRef`、`SystemTargetRef`与`PlatformRef`接受bounded workspace-relative locator，同时保留canonical
  slug简写；`KernelConfigRef`复用同一个词法路径规范化helper，但保持原有直接path语义。
- 单一`ConfigLoader`实现canonical存在性判定、fallback、containment、普通文件检查、读取与parse context；
  build与QEMU继续只消费同一resolved snapshot，并报告preset、target与Platform实际路径。
- BuildPreset/SystemTarget schema与example、CLI help、build-system guidance和`STM-RESOLVE-001`同步新规则；
  `conf list`仍只枚举tracked canonical target。
- resolver tests覆盖extensionless workspace-root配置图、canonical优先、`./`消歧、present-invalid fail-closed、
  missing双路径诊断、symlink逃逸和resolved path provenance；既有immutable-snapshot测试移除已漂移的
  `max_logical_cpus = 1`假设，继续用它本来已核验的defaulted OOM参数证明解析后不重读。

## Validation

- `just test xtask`：87/87通过，覆盖canonical/fallback/forced-path、nested locator、fail-closed、containment、
  provenance、selection compatibility与既有xtask回归。
- `just xtask qemu --preset qemu-virt-rv64-release --show-bindings`通过并报告canonical preset/target/Platform
  实际路径；`just xtask qemu --preset conf/build-presets/example.toml --show-bindings`与显式path target的完整
  tuple变体通过，证明两种CLI selection仍进入同一resolver且不启动QEMU。
- `just xtask conf list`通过并只列出tracked canonical target；`just fmt all --check`、`git diff --check`、
  两份变更schema的`jq empty`与`mdbook build docs`通过。
- Kernel build、QEMU guest、rootfs、KUnit、LTP、LA64/RV64 runtime与hardware：**Not Run**；本轮只改变host-side
  config locator，不以这些较宽路径替代resolver acceptance。

## Remaining Risk / Links

- canonical-first规则有意允许新增同名tracked canonical文件改变一个未加`./`的fallback输入含义；需要稳定锁定
  workspace文件时必须使用`./`或包含路径分隔符的显式路径。resolver通过实际路径日志暴露本次选择。
- Current contract：[`STM-RESOLVE-001`](../../contracts/configuration/system-target.md#stm-resolve-001--resolved-build-是不可手写的派生-snapshot)。
- Register / limitation / RFC / transaction / external source：None。
- Implementation evidence：本change对应的单一Git commit。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 baseline | 新规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `STM-RESOLVE-001` | Refine | BuildPreset、SystemTarget与Platform strict slug唯一定位到各自`conf/`目录 | canonical slug优先；仅目录项不存在时fallback到workspace-relative input；`./`强制path；present-invalid fail closed | resolver source/tests、build/QEMU诊断、current contract与本commit |

代码、测试、CLI/schema说明与current contract在同一个commit完成原子cutover；任一验证失败时不提交该commit。
