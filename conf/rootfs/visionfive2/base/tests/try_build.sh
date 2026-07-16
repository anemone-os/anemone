#!/bin/sh

echo Validating gcc installation...
gcc --version

echo Validating gcc help...
gcc --help

echo Validating gcc compilation...

echo Compiling c-test.c and test-lib.c into c-test executable...

gcc -v -o c-test c-test.c test-lib.c

# Temporary Anemone compatibility bridge: the current umask syscall stub
# reports 0777, so GNU ld leaves successful output files at mode 0666. Remove
# this chmod once umask is stored, inherited, and applied with Linux semantics.
chmod 0755 c-test

echo Running c-test program...
./c-test

# echo Compiling xv6
# 
# make -C xv6/xv6-riscv

echo All tests passed successfully!
