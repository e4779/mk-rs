# TEST_MAIN is set by the test harness.
# Test that environment variables are epxanded.
test2.mk.o: $TEST_MAIN
	secondprocess $prereq $stem $target

# TEST_MAIN is mk
mk:
	process bar $target

