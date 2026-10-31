# Helpers for the repository's policies. conftest runs them with --combine (tools/policy.sh), so
# `input` is every file it was given, each as {path, contents}, paths from the repository root.
package lib

# The parsed contents of the file at `path`.
file(path) := f.contents if {
	some f in input
	f.path == path
}

# conftest gives a YAML file whose top level is a list as one input per item.
items(path) := {f.contents | some f in input; f.path == path}

# Every path conftest was given that matches `pattern`.
paths(pattern) := {f.path | some f in input; glob.match(pattern, ["/"], f.path)}

# The folder names of the apps in apps/: TypeScript ones have a package.json, Rust ones a Cargo.toml.
ts_apps := {split(p, "/")[1] | some p in paths("apps/*/package.json")}

rust_apps := {split(p, "/")[1] | some p in paths("apps/*/Cargo.toml")}

apps := ts_apps | rust_apps

# The manifest of every project CI has to cover: apps, packages, Rust crates and plugins, vendored crates.
manifests := ((((paths("apps/*/package.json") | paths("apps/*/Cargo.toml")) | paths("packages/*/package.json")) | paths("crates/*/Cargo.toml")) | paths("plugins/*/Cargo.toml")) | paths("vendor/*/Cargo.toml")

# The files the repository tracks, which tools/policy.sh lists as {"files": [...]} in
# tracked-files.json.
tracked_files := f.contents.files if {
	some f in input
	regex.match(`(^|/)tracked-files\.json$`, f.path)
}

# The files the Rust code embeds from outside its own crate, which tools/policy.sh lists as
# {"files": [...]} in embedded-files.json.
embedded_files := f.contents.files if {
	some f in input
	regex.match(`(^|/)embedded-files\.json$`, f.path)
}

# A value that may be one string or a list of them, as a set.
strings(x) := {x} if is_string(x)

strings(x) := {s | some s in x} if is_array(x)

# Whether a glob as GitHub tools write it (picomatch, minimatch) matches `path`. OPA's glob needs
# a separator on each side of `**`, where those let `**/` match no folders at all, so the
# patterns are tried with those folders taken out too.
matches(pattern, path) if glob.match(pattern, ["/"], path)

matches(pattern, path) if {
	startswith(pattern, "**/")
	glob.match(substring(pattern, 3, -1), ["/"], path)
}

matches(pattern, path) if {
	contains(pattern, "/**/")
	glob.match(replace(pattern, "/**/", "/"), ["/"], path)
}
