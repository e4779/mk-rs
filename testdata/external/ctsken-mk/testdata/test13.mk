# Assorted expansion types.
targets = rupert ruxpin
targetpath = ./testdata
joinedbincmds = ${targets:%=$targetpath/%}
suffix = o
prefix = name
suffixed = ${targets:%=%.$suffix}
suffixes = o ab d
multisuffixed = ${targets:%=%.$suffixes}

# all:V: $joinedbincmds
all:V: 
	process1 $joinedbincmds
	process2 ab$targetpath
	process3 $targetpathab
	process4 $targetpath/foo
	process5 ${targetpath}/foo
	process6 "$targetpath"
	process7 $prefix.$suffix
	process8 $suffixed
	process9 $multisuffixed

