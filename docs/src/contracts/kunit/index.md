# KUnit 当前契约

**Owner：** KUnit boot runner与repository validation policy
**覆盖范围：** in-kernel unit case的执行环境、cleanup/failure边界、live scheduling例外、proof外推和production代码形状
**不覆盖：** host test、用户态integration/LTP、benchmark、独立single-case death test或各production subsystem自身的correctness contract
**最后核验：** 2026-08-11

本目录只约束KUnit作为boot-integrated unit test及其证明机制的共同边界，不把各subsystem被测语义迁入KUnit owner。

## Contract Surfaces

- [Execution and proof](./execution-and-proof.md)：runner环境、cleanup、并发握手、证据外推与production shape。

## 邻接契约

- [Scheduler当前契约](../scheduler/index.md)：KUnit若测试真实wait/wake，仍必须服从production scheduler协议。
- [Task当前契约](../task/index.md)：KUnit使用production kthread/kworker时依赖其真实lifecycle，不另建测试线程模型。
- [Time当前契约](../time/index.md)：timer/timekeeping测例可以把时间推进作为被测语义，但不能借此证明无关调度交错。
