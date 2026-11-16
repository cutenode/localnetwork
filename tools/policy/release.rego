# The release archives ship the third-party notices: `cargo notices` (.cargo/config.toml) writes
# them where dist-workspace.toml's include takes them from, for every platform dist builds, and
# about.toml accepts the licences deny.toml allows, so a crate cargo-deny lets in can't stop a
# release at cargo-about.
package release

import data.lib

dist := lib.file("dist-workspace.toml").dist

about := lib.file("about.toml")

# Where `cargo notices` writes the notices.
notices_file := args[i + 1] if {
	args := lib.file(".cargo/config.toml").alias.notices
	some i, arg in args
	arg == "--output-file"
}

deny contains msg if {
	not notices_file
	msg := ".cargo/config.toml: put back the `notices` alias with its --output-file; it writes the third-party notices the release archives include"
}

deny contains msg if {
	not notices_file in lib.string_set(object.get(dist, "include", []))
	msg := sprintf("dist-workspace.toml: add \"%s\" to include, so the release archives and the Homebrew formula ship the third-party notices", [notices_file])
}

deny contains msg if {
	some target in dist.targets
	not target in about.targets
	msg := sprintf("about.toml: add \"%s\" to targets; dist builds for it", [target])
}

accepted := {l | some l in about.accepted}

allowed := {l | some l in lib.file("deny.toml").licenses.allow}

deny contains msg if {
	some l in (allowed - accepted)
	msg := sprintf("about.toml: add \"%s\" to accepted, as deny.toml allows it", [l])
}

deny contains msg if {
	some l in (accepted - allowed)
	msg := sprintf("about.toml: take \"%s\" out of accepted, or allow it in deny.toml too", [l])
}
