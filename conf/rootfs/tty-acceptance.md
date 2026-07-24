# TTY RV64 acceptance rootfs

`tty-acceptance-rv64.toml`只引用仓库内ignored staging路径，不拥有外部BusyBox或测试盘。
使用者必须通过repository wrapper显式传入两份只读输入：

```sh
./scripts/run-tty-test-rv64.sh \
  --busybox /path/to/rv64-musl-busybox \
  --sdcard /path/to/rv64-sdcard-master \
  --mode auto \
  --log build/tty-stage4-rv64.log
```

wrapper在复制前验证BusyBox是static RISC-V ELF；不限制其版本、构建来源或artifact identity。
launcher在任何acceptance case前核对测试实际依赖的`ash/sleep/stty/vi` applet，缺失时fail closed。
原件不被修改；运行副本分别位于
`build/tty-acceptance/staging/riscv64/busybox`与
`build/runtime/tty-acceptance-rv64/disk-x0.img`。wrapper使用显式
`qemu-virt-rv64-pretest-release` preset，并把kernel、测试盘副本与acceptance rootfs分别绑定到
tracked Platform的`kernel-image`、`disk-x0`与`disk-x1`，不读取或切换root `kconfig`中的build selection。

## 人工vi checklist

使用与自动matrix相同的commit、platform、BusyBox路径和测试盘master，把`--mode`改为`vi`并保留完整log。
launcher显示人工提示后：

1. 等待vi首屏显示，确认显示区域为80x24，能够即时响应且没有双重echo、阶梯换行或重复CR。
2. 进入insert mode，输入第一行`alpha`；第二行先输入一个多余字符，用Backspace删除，再完成`beta`。
3. 按Esc，输入`:wq`并回车；launcher必须报告`TTYTEST:PASS:manual-vi`。
4. `manual-erase`提示后输入`ab`、按Backspace、输入`c`并回车，期望record为`ac\n`。
5. `manual-kill`提示后输入`abc`、按Ctrl-U、输入`d`并回车，期望record为`d\n`。
6. `manual-eof`提示后输入`xy`并按Ctrl-D，期望不含delimiter的`xy` record。
7. 确认每项PASS、退出vi后canonical+echo snapshot恢复，最终出现`TTYTEST:SUMMARY:PASS`并正常关机。

人工观察只证明RV64 user-run checklist，不能替代自动matrix；R1中LA64 compile/runtime为Not Run，RV64结果不得
外推到LA64。

## 人工ash job-control checklist

使用与自动matrix相同的commit、platform、BusyBox路径和测试盘master，把`--mode`改为`jobctl`，日志写入
`build/tty-stage4-rv64-jobctl.log`。出现`TTYTEST:MANUAL:ASH:launcher-ready`和ash prompt后：

1. 确认启动期间没有`job control turned off`。
2. 执行`/bin/busybox sleep 30`，按Ctrl-C；确认foreground job终止并返回prompt。
3. 再执行`/bin/busybox sleep 30`，按Ctrl-Z，运行`jobs`，确认job为Stopped。
4. 运行`fg`，再次按Ctrl-Z；运行`bg`与`jobs`，确认job在background继续运行。
5. 运行`fg`并按Ctrl-C，确认终止job并返回prompt。
6. 执行`/bin/busybox cat`，按Ctrl-Z后运行`bg`；确认background read因`SIGTTIN`再次停止且`jobs`可见Stopped。
7. 运行`fg`，输入一行并回车，按Ctrl-C结束`cat`，确认输入没有被background read提前消费。
8. 输入`exit`；launcher必须报告`TTYTEST:PASS:manual-ash-jobctl`，随后出现`TTYTEST:SUMMARY:PASS`并正常关机。

这份人工证据只覆盖无法稳定自动判定的RV64 ash交互交接；它不替代自动signal/access matrix、KUnit、source audit，
也不证明LA64、hardware或LTP。
