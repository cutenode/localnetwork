package release_test

import data.fixtures
import data.release

test_fixture_passes if {
	count(release.deny) == 0 with input as fixtures.repo
}

test_no_notices_alias if {
	denied := release.deny with input as fixtures.with_file(".cargo/config.toml", {"alias": {}})
	denied == {".cargo/config.toml: put back the `notices` alias with its --output-file; it writes the third-party notices the release archives include"}
}

test_notices_not_included if {
	broken := json.remove(fixtures.file("dist-workspace.toml"), ["dist/include"])
	denied := release.deny with input as fixtures.with_file("dist-workspace.toml", broken)
	denied == {"dist-workspace.toml: add \"target/NOTICES.md\" to include, so the release archives and the Homebrew formula ship the third-party notices"}
}

test_target_left_out if {
	broken := json.patch(fixtures.file("dist-workspace.toml"), [{"op": "add", "path": "/dist/targets/-", "value": "x86_64-pc-windows-msvc"}])
	denied := release.deny with input as fixtures.with_file("dist-workspace.toml", broken)
	denied == {"about.toml: add \"x86_64-pc-windows-msvc\" to targets; dist builds for it"}
}

test_licence_only_allowed if {
	broken := json.patch(fixtures.file("deny.toml"), [{"op": "add", "path": "/licenses/allow/-", "value": "Zlib"}])
	denied := release.deny with input as fixtures.with_file("deny.toml", broken)
	denied == {"about.toml: add \"Zlib\" to accepted, as deny.toml allows it"}
}

test_licence_only_accepted if {
	broken := json.patch(fixtures.file("about.toml"), [{"op": "add", "path": "/accepted/-", "value": "Zlib"}])
	denied := release.deny with input as fixtures.with_file("about.toml", broken)
	denied == {"about.toml: take \"Zlib\" out of accepted, or allow it in deny.toml too"}
}
