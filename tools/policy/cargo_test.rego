package cargo_test

import data.cargo
import data.fixtures

test_fixture_passes if {
	count(cargo.deny) == 0 with input as fixtures.repo
}

test_app_not_a_member if {
	broken := {"workspace": {"members": ["crates/*", "plugins/*", "vendor/pdb"]}}
	"Cargo.toml: add \"apps/tool\" to the workspace members" in cargo.deny with input as fixtures.with_file("Cargo.toml", broken)
}

test_vendored_crate_not_a_member if {
	broken := {"workspace": {"members": ["apps/tool", "crates/*", "plugins/*"]}}
	"Cargo.toml: add \"vendor/pdb\" to the workspace members" in cargo.deny with input as fixtures.with_file("Cargo.toml", broken)
}

test_field_not_inherited if {
	crate := fixtures.file("crates/loclib-x/Cargo.toml")
	broken := json.patch(crate, [{"op": "replace", "path": "/package/edition", "value": "2021"}])
	"crates/loclib-x/Cargo.toml: set `edition.workspace = true`" in cargo.deny with input as fixtures.with_file("crates/loclib-x/Cargo.toml", broken)
}

test_vendored_crate_keeps_its_edition if {
	vendored := fixtures.file("vendor/pdb/Cargo.toml")
	broken := json.remove(vendored, ["package/rust-version"])
	denied := cargo.deny with input as fixtures.with_file("vendor/pdb/Cargo.toml", broken)
	denied == {"vendor/pdb/Cargo.toml: set `rust-version.workspace = true`"}
}

test_no_workspace_lints if {
	plugin := fixtures.file("plugins/p/Cargo.toml")
	broken := json.remove(plugin, ["lints"])
	"plugins/p/Cargo.toml: add `[lints] workspace = true`, so the workspace's lints apply" in cargo.deny with input as fixtures.with_file("plugins/p/Cargo.toml", broken)
}

test_plugin_misnamed if {
	plugin := fixtures.file("plugins/p/Cargo.toml")
	broken := json.patch(plugin, [{"op": "replace", "path": "/package/name", "value": "p"}])
	"plugins/p/Cargo.toml: name it locallibrary-plugin-p; nextest's plugins profile selects plugins by name" in cargo.deny with input as fixtures.with_file("plugins/p/Cargo.toml", broken)
}

test_plugin_published if {
	plugin := fixtures.file("plugins/p/Cargo.toml")
	broken := json.remove(plugin, ["package/publish"])
	"plugins/p/Cargo.toml: set `publish = false`; plugins ship inside the locallibrary binary" in cargo.deny with input as fixtures.with_file("plugins/p/Cargo.toml", broken)
}

test_crate_without_readme if {
	crate := fixtures.file("crates/loclib-x/Cargo.toml")
	broken := json.remove(crate, ["package/readme"])
	"crates/loclib-x/Cargo.toml: set `readme = \"README.md\"`, the crate's docs" in cargo.deny with input as fixtures.with_file("crates/loclib-x/Cargo.toml", broken)
}

without_dev_package(name) := fixtures.with_file("Cargo.toml", json.remove(fixtures.file("Cargo.toml"), [sprintf("profile/dev/package/%s", [name])]))

test_dev_package_dropped if {
	"Cargo.toml: put back [profile.dev.package.sha2]; `cargo remove` deletes it (git diff Cargo.toml shows what went)" in cargo.deny with input as without_dev_package("sha2")
}

test_dev_package_gone_from_lock if {
	lock := {"path": "/scratch/Cargo.lock.toml", "contents": {"package": [{"name": "tool"}]}}
	count(cargo.deny) == 0 with input as array.concat([f | some f in without_dev_package("sha2"); f.path != lock.path], [lock])
}

test_dev_package_not_listed if {
	crate := fixtures.file("Cargo.toml")
	broken := json.patch(crate, [{"op": "add", "path": "/profile/dev/package/digest", "value": {"opt-level": 3}}])
	"Cargo.toml: add \"digest\" to dev_optimised in tools/policy/cargo.rego, so the policies notice if it goes missing" in cargo.deny with input as fixtures.with_file("Cargo.toml", broken)
}

crate_x := fixtures.file("crates/loclib-x/Cargo.toml")

test_published_crate_without_all_features if {
	broken := json.remove(crate_x, ["package/metadata/docs/rs/all-features"])
	"crates/loclib-x/Cargo.toml: set `all-features = true` in [package.metadata.docs.rs], so docs.rs shows what features add" in cargo.deny with input as fixtures.with_file("crates/loclib-x/Cargo.toml", broken)
}

test_published_crate_without_docsrs_cfg if {
	broken := json.patch(crate_x, [{"op": "replace", "path": "/package/metadata/docs/rs/rustdoc-args", "value": ["--cfg", "other"]}])
	"crates/loclib-x/Cargo.toml: set `rustdoc-args = [\"--cfg\", \"docsrs\"]` in [package.metadata.docs.rs], so docs.rs marks what each feature adds" in cargo.deny with input as fixtures.with_file("crates/loclib-x/Cargo.toml", broken)
}

loclib_x_attributes := fixtures.file("/scratch/crate-roots.json").roots["crates/loclib-x/Cargo.toml"].attributes

test_published_crate_without_missing_docs if {
	roots := {"roots": {"crates/loclib-x/Cargo.toml": {"root": "crates/loclib-x/src/lib.rs", "attributes": ["#![cfg_attr(docsrs, feature(doc_cfg))]"]}}}
	denied := cargo.deny with input as fixtures.with_file("/scratch/crate-roots.json", roots)
	denied == {"crates/loclib-x/Cargo.toml: add `#![warn(missing_docs)]` to crates/loclib-x/src/lib.rs, so clippy, which CI runs with -D warnings, fails on a public item without docs"}
}

# Clippy's own missing_docs_in_private_items isn't missing_docs.
test_published_crate_allowing_missing_docs if {
	attributes := array.concat(loclib_x_attributes, [
		"#![allow(missing_docs)]",
		"#![cfg_attr(test, expect(dead_code, missing_docs))]",
		"#![allow(clippy::missing_docs_in_private_items)]",
	])
	roots := {"roots": {"crates/loclib-x/Cargo.toml": {"root": "crates/loclib-x/src/lib.rs", "attributes": attributes}}}
	denied := cargo.deny with input as fixtures.with_file("/scratch/crate-roots.json", roots)
	denied == {
		"crates/loclib-x/Cargo.toml: take `#![allow(missing_docs)]` out of crates/loclib-x/src/lib.rs, and document what it hid",
		"crates/loclib-x/Cargo.toml: take `#![cfg_attr(test, expect(dead_code, missing_docs))]` out of crates/loclib-x/src/lib.rs, and document what it hid",
	}
}

# The root is wherever the crate's [lib] puts it, as crate-roots.json says.
test_published_crate_with_its_own_root if {
	roots := {"roots": {"crates/loclib-x/Cargo.toml": {"root": "crates/loclib-x/lib.rs", "attributes": loclib_x_attributes}}}
	count(cargo.deny) == 0 with input as fixtures.with_file("/scratch/crate-roots.json", roots)
}

# An app that goes to crates.io needs the same as a crate, at its own root.
test_published_app_with_its_own_root if {
	app := json.patch(fixtures.file("apps/tool/Cargo.toml"), [
		{"op": "remove", "path": "/package/publish"},
		{"op": "add", "path": "/package/metadata", "value": {"docs": {"rs": {"all-features": true, "rustdoc-args": ["--cfg", "docsrs"]}}}},
	])
	roots := {"roots": {"apps/tool/Cargo.toml": {"root": "apps/tool/lib.rs", "attributes": []}}}
	repo := [f | some f in fixtures.with_file("apps/tool/Cargo.toml", app); f.path != "/scratch/crate-roots.json"]
	denied := cargo.deny with input as array.concat(repo, [{"path": "/scratch/crate-roots.json", "contents": roots}])
	denied == {
		"apps/tool/Cargo.toml: add `#![warn(missing_docs)]` to apps/tool/lib.rs, so clippy, which CI runs with -D warnings, fails on a public item without docs",
		"apps/tool/Cargo.toml: add `#![cfg_attr(docsrs, feature(doc_cfg))]` to apps/tool/lib.rs, so docs.rs marks what each feature adds",
	}
}

# A package without a library has no crate attributes to check.
test_published_crate_without_a_library if {
	count(cargo.deny) == 0 with input as fixtures.with_file("/scratch/crate-roots.json", {"roots": {}})
}

test_unpublished_crate_needs_neither if {
	unpublished := json.patch(crate_x, [{"op": "remove", "path": "/package/metadata"}, {"op": "add", "path": "/package/publish", "value": false}])
	repo := [f | some f in fixtures.with_file("crates/loclib-x/Cargo.toml", unpublished); f.path != "/scratch/crate-roots.json"]
	count(cargo.deny) == 0 with input as array.concat(repo, [{"path": "/scratch/crate-roots.json", "contents": {"roots": {}}}])
}

test_array_alias if {
	broken := {"alias": {"notices": "about generate --output-file target/NOTICES.md about.hbs", "fuzz-x": ["fuzz", "run", "x"]}}
	denied := cargo.deny with input as fixtures.with_file(".cargo/config.toml", broken)
	denied == {".cargo/config.toml: write the `fuzz-x` alias as one string (\"a b c\"), not an array; cargo appends a parent directory's array alias to it"}
}
