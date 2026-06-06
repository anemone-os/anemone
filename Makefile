all:
	bash ./build_all.sh

local: local_build local_test

local_build:
	docker exec -u root -w /workspaces/anemone gallant_lamarr make all
	bash ./rcopy.sh
	mkdir -p etc

local_test:
	just xtask qemu --platform pre-test-la64 --image kernel-la | tee etc/log-la.log
	just xtask qemu --platform pre-test-rv64 --image kernel-rv | tee etc/log-rv.log

	@echo PASS COUNT ON RISC-V:
	@cat etc/log-rv.log | grep "TPASS" | wc -l
	@echo PASS COUNT ON LA:
	@cat etc/log-la.log | grep "TPASS" | wc -l

.PHONY: all local_test local_build