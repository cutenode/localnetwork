# Each ADR's number is its own within its folder (docs/adr/, an app's docs/adr/): the next one
# takes the next free number.
package adr

import data.lib

# Every ADR as [path, folder, number]: docs/adr/0002-some-decision.md is in docs/adr, numbered 0002.
records contains [path, match[1], match[2]] if {
	some path in lib.tracked_files
	some match in regex.find_all_string_submatch_n(`^((?:.*/)?adr)/([0-9]+)-[^/]+\.md$`, path, 1)
}

deny contains msg if {
	some [_, folder, number] in records
	same := sort([path | some [path, f, n] in records; f == folder; n == number])
	count(same) > 1
	msg := sprintf("%s: %s share the number %s; give the newer one the next free number", [folder, concat(" and ", same), number])
}
