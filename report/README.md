# Anemone 竞赛交付文档

本目录集中维护面向竞赛提交与展示的文档。

- [`kernel-report/`](./kernel-report/)：内核技术报告，使用 Typst 编写。
- [`buildstorm/anemone-buildstorm-report.md`](./buildstorm/anemone-buildstorm-report.md)：BuildStorm 内核设计与优化报告，使用 Markdown 编写。
- [`ppt/`](./ppt/)：竞赛演示文稿及其素材。

内核技术报告可在仓库根目录执行以下命令构建：

```sh
make -C report/kernel-report
```

生成的 PDF 位于 `report/kernel-report/build/anemone-report.pdf`，不纳入版本控制。

BuildStorm 优化报告可在仓库根目录执行以下命令构建：

```sh
make -C report/buildstorm
```

该构建需要 Pandoc、Typst 与 `Noto Serif CJK SC` 字体。生成的 PDF 位于
`report/buildstorm/build/anemone-buildstorm-report.pdf`，正文后会依次合入四篇研究附录；
复现索引、原始数据、实验补丁和完整日志作为具体链接的随附证据交付。生成的 PDF 不纳入版本控制。
