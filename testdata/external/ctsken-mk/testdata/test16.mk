
<./testdata/deps9

test3.mk.o: $deps
	secondprocess $prereq2 $prereq1

one:
	process bar $extracmdarg

two:
	$extracmdarg rebar