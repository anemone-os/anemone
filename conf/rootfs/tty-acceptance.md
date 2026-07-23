# TTY Stage 2 RV64 acceptance rootfs

`tty-acceptance-rv64.toml`只引用仓库内ignored staging路径，不拥有外部BusyBox或测试盘。
使用者必须通过repository wrapper显式传入两份只读输入：

```sh
./scripts/run-tty-test-rv64.sh \
  --busybox /path/to/rv64-musl-busybox \
  --sdcard /path/to/rv64-sdcard-master \
  --mode auto \
  --log build/tty-stage2-rv64.log
```

wrapper在复制前验证BusyBox是static RISC-V ELF、SHA-256为
`fd9cb9dc66ba740dc94b055b564de0597453adfceef9be158b3774ca58b95241`；host有
`qemu-riscv64`时预检版本及`ash/stty/vi/mount/stat/poweroff` applet，host没有qemu-user时，launcher在任何
acceptance case前执行同一runtime核对并fail closed。原件不被修改；运行副本分别位于
`build/tty-acceptance/staging/riscv64/busybox`与worktree根目录`sdcard-rv.img`。

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
