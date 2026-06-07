# Variable deps is set in include.
# in P9P, . is the cwd of where we execute mk
<./testdata/deps6

test3.mk.o: $deps
	secondprocess ab

one:
	process bar

two:
	process rebar
