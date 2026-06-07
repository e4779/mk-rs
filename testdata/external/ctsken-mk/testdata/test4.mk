# A variable is expanded to decide to run two sub-depenencies
deps = one two

test3.mk.o: $deps
	secondprocess ab

one:
	process bar

two:
	process rebar
