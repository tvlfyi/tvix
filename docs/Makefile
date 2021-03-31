all: build

puml:
	plantuml *.puml -tsvg

html:
	pandoc *.md -f markdown --self-contained -t html -s -o tvix.html --csl=${CSL}

build: puml html

clean:
	rm -f *.tex *.pdf *.png *.svg
