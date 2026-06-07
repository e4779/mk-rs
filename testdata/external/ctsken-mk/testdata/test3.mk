# A variable is expanded to decide to run the second rule
deps = one

test3.mk.o: $deps
	secondprocess ab

one:
	process bar
