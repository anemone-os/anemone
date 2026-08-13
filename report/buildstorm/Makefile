PANDOC ?= pandoc

BUILD_DIR := build
PDF := $(BUILD_DIR)/anemone-buildstorm-report.pdf

SOURCES := \
	anemone-buildstorm-report.md \
	appendices/tlb-optimization.md \
	appendices/positive-dentry-residency.md \
	appendices/page-bounded-c-string-copy.md \
	appendices/candidate-screening.md

.PHONY: all

all: $(PDF)

$(PDF): $(SOURCES) page-breaks.lua captions.lua caption-style.typ Makefile
	mkdir -p $(BUILD_DIR)
	$(PANDOC) $(SOURCES) \
		--from=markdown \
		--standalone \
		--toc \
		--lua-filter=page-breaks.lua \
		--lua-filter=captions.lua \
		--include-in-header=caption-style.typ \
		--resource-path=. \
		--pdf-engine=typst \
		-V papersize=a4 \
		-V fontsize=10.5pt \
		-V mainfont='Noto Serif CJK SC' \
		-V lang=zh \
		-o $(PDF)
