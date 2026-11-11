package dependabot_test

import data.dependabot
import data.fixtures

tracked_with(files) := fixtures.with_file("/scratch/tracked-files.json", {"files": array.concat(fixtures.file("/scratch/tracked-files.json").files, files)})

test_fixture_passes if {
	count(dependabot.deny) == 0 with input as fixtures.repo
}

test_root_lockfile_by_directory if {
	count(dependabot.deny) == 0 with input as tracked_with(["Cargo.lock"])
}

test_lockfile_not_updated if {
	denied := dependabot.deny with input as tracked_with(["apps/tool/bench/package-lock.json"])
	denied == {".github/dependabot.yml: add /apps/tool/bench to the npm entry's directories, so Dependabot updates apps/tool/bench/package-lock.json"}
}

test_other_ecosystem_does_not_count if {
	denied := dependabot.deny with input as tracked_with(["apps/site/Cargo.lock"])
	denied == {".github/dependabot.yml: add /apps/site to the cargo entry's directories, so Dependabot updates apps/site/Cargo.lock"}
}
