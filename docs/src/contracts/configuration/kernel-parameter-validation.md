# Kernel 参数合法性当前契约

**Contract ID：** `KCONFIG-VALIDATION-001`
**状态：** Active
**Owner：** KernelConfig 拥有选定参数值；使用参数的 kernel subsystem 拥有其语义合法性
**参与领域：** KernelConfig / `scripts/xtask` config loader与generator / kernel consumers
**覆盖范围：** KernelConfig 参数及其它直接注入 kernel、且合法性只取决于 kernel consumer 的 build-time 参数
**不覆盖：** 配置文件定位与引用安全、跨配置对象解析、host artifact/command、Platform/SystemTarget 自身 schema、
runtime 用户输入，或对全部既有参数是否需要额外约束的清单式审计
**实现位置：** `scripts/xtask/src/config/kconfig.rs`、`scripts/xtask/src/config/resolve.rs`、
`scripts/xtask/src/tasks/build/generated_defs.rs` 与各 kernel consumer
**依赖：** [`STM-OWNER-001`](./system-target.md#stm-owner-001--每个配置事实只有一个规范-owner)
**Pending Successor：** None
**最后核验：** 2026-08-05

本页建立参数传输与参数语义之间的稳定 owner boundary。它不要求每个数值都有额外范围；是否存在
非零、范围、幂次、容量、对齐、溢出或跨字段约束，由真正使用该值并承担错误后果的 kernel owner 决定。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| 选定的 feature / policy / capacity 值 | KernelConfig | resolved immutable projection | 表达本次 kernel build 输入 |
| TOML 语法、closed schema、字段类型与缺省值完整性 | `scripts/xtask` config loader | parsed/materialized value | 建立可生成的完整输入 |
| generated Rust definition | `scripts/xtask` generator | kernel 只读编译输入 | 忠实传输已选参数，不重新解释 |
| 参数的 kernel semantic predicate | 使用该参数的 kernel subsystem | generated typed constant | 决定该 consumer 可接受的值域与字段关系 |
| semantic admission failure | kernel compilation | compiler diagnostic | 阻止非法参数形成可运行 kernel artifact |

生成定义是 transport projection，不是第二个参数 owner。xtask 能够成功解析并生成某个值，只表示它满足
配置表示与完整性要求，不表示 kernel 已接受该值的语义。

## KCONFIG-VALIDATION-001 — Kernel consumer 唯一定义参数语义合法性

**规则：** `scripts/xtask` 只负责反序列化、closed-schema/字段类型等基本格式检查、缺省值物化，以及把
resolved 参数忠实生成给 kernel。它可以拒绝 malformed TOML、未知字段、类型或 representation variant
不匹配、缺少无法物化的必需值，以及不安全的配置文件引用；不得根据 kernel consumer 语义拒绝、截断、
clamp、fallback 或改写参数。

参数的范围、零/非零、幂次、容量、大小/对齐、算术溢出和跨字段关系只由相关 kernel subsystem 定义。
需要约束时，owner 应优先在消费点附近使用 `static_assert!`、const construction 或等价编译期手段，使非法
配置在 kernel compilation 中失败；不得把同一 predicate 复制到 xtask，也不得以 runtime fallback 把非法
build 输入变成另一种行为。多个 kernel consumer 各自只声明自己承担的约束；真正共享的 predicate 应落在
它们的最低共同 kernel owner，而不是回流到 generator。

**违反表现：** xtask 检查 queue size 是否为二次幂、比较两个容量字段、把零值替换成默认值，或为了提前给出
错误而复制 kernel `static_assert!`；kernel 在运行时才发现固定 build 参数非法并静默降级；测试只搜索 authored
generator template 中是否出现某条 `pub const`，却把结果当作参数语义或传输正确性的证明。

**验证 / Enforcement：** xtask 的定向测试只覆盖非平凡的解析、物化、引用和生成机制，不为模板中肉眼可见的
常量逐项建立字符串存在性测试。新增或修改带 semantic predicate 的参数时，默认 KernelConfig 必须完成 kernel
build，并使用代表性非法配置确认 kernel compilation 在 owner-local 编译期检查处失败；该失败不得由 xtask
parser/generator 提前制造。普通源码审查确认 generator 未引入 semantic predicate、clamp 或 fallback。

**当前来源：** 2026-08-05 parameter-validation Patch 及其 Git 提交；本次 contract cutover 不执行全量既有
参数断言审计，也不因此声称每个数值参数都需要额外约束。
