# TEST_MAIN is set by the test harness.
# Test that mkfile variables are expanded in backquote substitution.

hello = one two three
deps = `echo $hello`

test2.mk.o:
	process $deps
