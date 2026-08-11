# 复现与结果证据

本目录保存 BuildStorm 优化报告采用的公开复现信息和结果摘要，包括原版本与优化版本的 Git 提交或固定补丁、运行配置、命令、正确性检查和实验结果。

- [`results.csv`](./results.csv)：正文统计图与结果表的公开数据源；`source_identity` 与 `identity_kind` 区分 Git 提交和固定补丁，不同测试配置的结果不可相加或直接排名。
- [复现身份索引](./reproducibility.md)：每项结果对应的版本、配置、命令、检查条件和适用范围。
- [`user-fault-local-tlb.patch`](./patches/user-fault-local-tlb.patch)：无法仅由 Git 提交表达的固定实验补丁；文件内容的 SHA-256 同时用于确认补丁未发生变化。
- [`buildstorm-rv64-smp8.log`](./logs/buildstorm-rv64-smp8.log)：RV64 八核 BuildStorm 完整运行日志。
- [`buildstorm-la64-smp8.log`](./logs/buildstorm-la64-smp8.log)：LA64 八核 BuildStorm 完整运行日志。

正文、附录与这里的公开证据可以独立阅读和核验，不依赖团队内部研究目录。
