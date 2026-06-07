
prereq.(\w+):R:
	echo $stem1 $(echo ${stem1})

proto.txt: prereq.txt
	echo $target