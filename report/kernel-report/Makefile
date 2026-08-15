PDF := build/anemone-report.pdf

.PHONY: all clean

all: $(PDF)

$(PDF): main.typ conf.typ components/*.typ content/*.typ
	mkdir -p build
	typst compile main.typ $(PDF)

clean:
	rm -rf build
