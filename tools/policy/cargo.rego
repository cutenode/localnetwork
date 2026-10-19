# Rust packages are workspace members that share its settings (.github/NEW_PROJECT.md, "A new
# Rust app", "A locallibrary plugin" and "Shared libraries").
package cargo

import data.lib

workspace := lib.file("Cargo.toml").workspace

# The members' manifests: apps/<app>/, crates/*, plugins/* and vendor/*.
members := {path | some path in lib.manifests; endswith(path, "/Cargo.toml")}

# crates/* and plugins/* join through globs; apps and vendored crates have to be listed.
deny contains msg if {
	some path in members
	dir := trim_suffix(path, "/Cargo.toml")
	not dir in {m | some m in workspace.members}
	not startswith(dir, "crates/")
	not startswith(dir, "plugins/")
	msg := sprintf("Cargo.toml: add \"%s\" to the workspace members", [dir])
}

# Fields every member takes from [workspace.package]. Vendored crates keep their own edition,
# licence and repository, which say where they came from.
inherited(path) := {"edition", "rust-version", "license", "repository"} if not startswith(path, "vendor/")

inherited(path) := {"rust-version"} if startswith(path, "vendor/")

deny contains msg if {
	some path in members
	some field in inherited(path)
	not lib.file(path)["package"][field].workspace
	msg := sprintf("%s: set `%s.workspace = true`", [path, field])
}

deny contains msg if {
	some path in members
	not lib.file(path).lints.workspace
	msg := sprintf("%s: add `[lints] workspace = true`, so the workspace's lints apply", [path])
}

deny contains msg if {
	some path in lib.paths("plugins/*/Cargo.toml")
	id := split(path, "/")[1]
	pkg := lib.file(path)["package"]
	pkg.name != sprintf("locallibrary-plugin-%s", [id])
	msg := sprintf("%s: name it locallibrary-plugin-%s; nextest's plugins profile selects plugins by name", [path, id])
}

deny contains msg if {
	some path in lib.paths("plugins/*/Cargo.toml")
	not lib.file(path)["package"].publish == false
	msg := sprintf("%s: set `publish = false`; plugins ship inside the locallibrary binary", [path])
}

# A loclib crate's README is its crate docs.
deny contains msg if {
	some path in lib.paths("crates/loclib-*/Cargo.toml")
	object.get(lib.file(path)["package"], "readme", "") != "README.md"
	msg := sprintf("%s: set `readme = \"README.md\"`, the crate's docs", [path])
}
