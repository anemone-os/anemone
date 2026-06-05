all:
	cp conf/.benchconf kconfig
	just conf switch rv
	just build
	cp build/anemone.elf kernel-rv
	just conf switch la
	just build
	cp build/anemone.elf kernel-la
	
.PHONY: all