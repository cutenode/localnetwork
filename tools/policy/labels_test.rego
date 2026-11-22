package labels_test

import data.fixtures
import data.labels

labeler := fixtures.file("/scratch/labeler.json")

test_fixture_passes if {
	count(labels.deny) == 0 with input as fixtures.repo
}

test_labeler_label_not_defined if {
	broken := object.union(labeler, {"extra": []})
	".github/labeler/: \"extra\" isn't in .github/labels.yml" in labels.deny with input as fixtures.with_file("/scratch/labeler.json", broken)
}

test_app_without_label if {
	repo := [f | some f in fixtures.repo; f.contents != {"name": "tool"}]
	".github/labels.yml: add a label for tool" in labels.deny with input as repo
}

test_app_label_misses_its_files if {
	broken := object.union(labeler, {"tool": [{"changed-files": [{"any-glob-to-any-file": "crates/**"}]}]})
	".github/labeler/: the \"tool\" label should match apps/tool/Cargo.toml" in labels.deny with input as fixtures.with_file("/scratch/labeler.json", broken)
}

test_manifest_names_unknown_label if {
	manifest := fixtures.file("crates/loclib-x/Cargo.toml")
	broken := json.patch(manifest, [{"op": "replace", "path": "/package/metadata/localnetwork/labels", "value": ["tool: core", "tool: nope"]}])
	repo := [f | some f in fixtures.with_file("crates/loclib-x/Cargo.toml", broken); f.path != "/scratch/labeler.json"]
	built := object.union(labeler, {"tool: nope": []})
	denied := labels.deny with input as array.concat(repo, [{"path": "/scratch/labeler.json", "contents": built}])
	"crates/loclib-x/Cargo.toml: \"tool: nope\" isn't in .github/labels.yml" in denied
	not ".github/labeler/: \"tool: nope\" isn't in .github/labels.yml" in denied
	not "crates/loclib-x/Cargo.toml: \"tool: core\" isn't in .github/labels.yml" in denied
}

test_bug_form_without_label if {
	broken := {"labels": ["bug"]}
	".github/ISSUE_TEMPLATE/tool-bug.yml: add the \"tool\" label" in labels.deny with input as fixtures.with_file(".github/ISSUE_TEMPLATE/tool-bug.yml", broken)
}

test_no_bug_form if {
	".github/ISSUE_TEMPLATE: add tool-bug.yml" in labels.deny with input as fixtures.without_file(".github/ISSUE_TEMPLATE/tool-bug.yml")
}

test_tool_not_in_dropdown if {
	broken := {"body": [{"id": "tool", "attributes": {"options": ["site"]}}]}
	".github/ISSUE_TEMPLATE/feature_request.yml: add \"tool\" to the \"Which tool?\" options" in labels.deny with input as fixtures.with_file(".github/ISSUE_TEMPLATE/feature_request.yml", broken)
}

test_website_is_named_in_dropdowns if {
	labels.display("web") == "Local Network (the website)"
	labels.display("tool") == "tool"
}

test_file_without_label if {
	files := {"files": ["apps/site/a.ts", "apps/other/a.ts", "apps/x/README.md"]}
	denied := labels.deny with input as fixtures.with_file("/scratch/tracked-files.json", files)
	"apps/other/a.ts: no label in .github/labeler/ matches it" in denied
	"apps/x/README.md: no label in .github/labeler/ matches it" in denied
	not "apps/site/a.ts: no label in .github/labeler/ matches it" in denied
}

test_all_files_entry_labels_its_file if {
	files := {"files": ["docs/b.txt"]}
	count(labels.deny) == 0 with input as fixtures.with_file("/scratch/tracked-files.json", files)
}

test_area_label_without_rule if {
	repo := array.concat(fixtures.repo, [{"path": ".github/labels.yml", "contents": {"name": "tool: cli"}}])
	".github/labeler/: add a file for the \"tool: cli\" label" in labels.deny with input as repo
}

test_code_file_without_area if {
	files := {"files": ["crates/x/src/lib.rs", "plugins/p/src/lib.rs", "apps/web/src/lib/testing/a.ts", "apps/site/src/app.ts"]}
	denied := labels.deny with input as fixtures.with_file("/scratch/tracked-files.json", files)
	"plugins/p/src/lib.rs: no area label in .github/labeler/ matches it" in denied
	not "crates/x/src/lib.rs: no area label in .github/labeler/ matches it" in denied
	not "apps/web/src/lib/testing/a.ts: no area label in .github/labeler/ matches it" in denied
	not "apps/site/src/app.ts: no area label in .github/labeler/ matches it" in denied
}
