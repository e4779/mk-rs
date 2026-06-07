PROG=mk

# Locations
DESTDIR=/usr/local
BINDIR=bin

# Tools
GOTOOL=go
INSTALL=install


# Customizations: overwrite the above variables in a local config.mk file
#<|cat config.mk 2>/dev/null || true

sources = expand.go graph.go lex.go mk.go recipe.go remote.go rules.go
all:V:	$PROG

test:V:
    $GOTOOL test

%.1: %.1.md
    pandoc -s -t man -o $target $prereq


$PROG: ${sources}
	$GOTOOL build

install:V: $PROG
	$INSTALL $PROG $DESTDIR/$BINDIR/$PROG

clean:V:
	rm -f $PROG
