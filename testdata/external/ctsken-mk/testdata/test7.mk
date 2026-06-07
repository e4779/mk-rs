# Test if variables are expanded in file names.
# deps6 defines deps = one two
depsfile = deps6

<./testdata/$depsfile

test3.mk.o: $deps
	secondprocess ab

one:
	process bar

two:
	process rebar
