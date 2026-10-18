package labels_test

import data.fixtures
import data.labels

labeler := fixtures.file(".github/labeler.yml")

test_fixture_passes if {
	count(labels.deny) == 0 with input as fixtures.repo
}

test_labeler_label_not_defined if {
	broken := object.union(labeler, {"extra": []})
	".github/labeler.yml: \"extra\" isn't in .github/labels.yml" in labels.deny with input as fixtures.with_file(".github/labeler.yml", broken)
}

test_app_without_label if {
	repo := [f | some f in fixtures.repo; f.contents != {"name": "tool"}]
	".github/labels.yml: add a label for tool" in labels.deny with input as repo
}

test_app_label_misses_its_files if {
	broken := object.union(labeler, {"tool": [{"changed-files": [{"any-glob-to-any-file": "crates/**"}]}]})
	".github/labeler.yml: the \"tool\" label should match apps/tool/Cargo.toml" in labels.deny with input as fixtures.with_file(".github/labeler.yml", broken)
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
	".github/ISSUE_TEMPLATE/question.yml: add \"tool\" to the \"Which tool?\" options" in labels.deny with input as fixtures.with_file(".github/ISSUE_TEMPLATE/question.yml", broken)
}

test_website_is_named_in_dropdowns if {
	labels.display("web") == "Local Network (the website)"
	labels.display("tool") == "tool"
}

test_file_without_label if {
	files := {"files": ["apps/site/a.ts", "apps/other/a.ts", "apps/x/README.md"]}
	denied := labels.deny with input as fixtures.with_file("/scratch/tracked-files.json", files)
	"apps/other/a.ts: no label in .github/labeler.yml matches it" in denied
	"apps/x/README.md: no label in .github/labeler.yml matches it" in denied
	not "apps/site/a.ts: no label in .github/labeler.yml matches it" in denied
}
