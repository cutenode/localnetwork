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
