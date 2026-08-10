# 复现与结果证据

本目录保存 BuildStorm 优化报告采用的公开、围绕具体结论组织的复现身份与结果摘要，包括
baseline/candidate commit 或固定 patch identity、运行配置、命令、正确性 oracle 和实验结果。

- [`results.csv`](./results.csv)：正文统计图与结果表的公开数据源；`source_identity` 与 `identity_kind` 区分 Git commit 和固定 patch，不同 `profile` 的结果不可相加或直接排名。
- [复现身份索引](./reproducibility.md)：每项结论对应的版本、配置、命令、oracle 与证据边界。
- [`patches/`](./patches/)：无法仅由 Git commit 表达的冻结候选 patch；文件内容的 SHA-256 同时作为实验身份。

这里保存经过筛选和中性化的结论身份；正文、附录与公开证据能够独立阅读和核验。
