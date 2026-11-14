package adr_test

import data.adr
import data.fixtures

tracked_with(files) := fixtures.with_file("/scratch/tracked-files.json", {"files": array.concat(fixtures.file("/scratch/tracked-files.json").files, files)})

test_fixture_passes if {
	count(adr.deny) == 0 with input as fixtures.repo
}

test_numbers_differ_per_folder if {
	count(adr.deny) == 0 with input as tracked_with(["docs/adr/0001-a.md", "docs/adr/0002-b.md", "apps/tool/docs/adr/0001-c.md", "docs/adr/README.md"])
}

test_shared_number if {
	denied := adr.deny with input as tracked_with(["apps/tool/docs/adr/0002-b.md", "apps/tool/docs/adr/0001-a.md", "apps/tool/docs/adr/0002-c.md"])
	denied == {"apps/tool/docs/adr: apps/tool/docs/adr/0002-b.md and apps/tool/docs/adr/0002-c.md share the number 0002; give the newer one the next free number"}
}
