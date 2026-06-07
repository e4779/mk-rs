# Variable extracmdarg and deps are set in include.
# Test if extracmdarg is set for recipes
<./testdata/deps6

test3.mk.o: $deps
	secondprocess ab

one:
	process bar $extracmdarg

two:
	$extracmdarg rebar
