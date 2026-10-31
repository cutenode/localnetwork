package ruleset_test

import data.fixtures
import data.ruleset

test_fixture_passes if {
	count(ruleset.deny) == 0 with input as fixtures.repo
}

test_check_not_required if {
	broken := {"rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "Site CI passed"}, {"context": "Rust passed"}, {"context": "Lint passed"}]}}]}
	".github/rulesets/protect-main.json: require \"Deps passed\", then apply it with tools/ruleset.sh" in ruleset.deny with input as fixtures.with_file(".github/rulesets/protect-main.json", broken)
}

queue := {"type": "merge_queue", "parameters": {"merge_method": "MERGE"}}

with_queue := fixtures.with_file(".github/rulesets/protect-main.json", json.patch(fixtures.file(".github/rulesets/protect-main.json"), [{"op": "add", "path": "/rules/-", "value": queue}]))

test_merge_queue_without_merge_group if {
	".github/workflows/rust.yml: add `on.merge_group`; the merge queue waits for \"Rust passed\", which never reports otherwise" in ruleset.deny with input as with_queue
}

test_merge_queue_with_merge_group if {
	repo := [f |
		some f in with_queue
		not startswith(f.path, ".github/workflows/")
	]
	ci := {"path": ".github/workflows/ci.yml", "contents": fixtures.workflow({"pull_request": null, "merge_group": null}, "Rust passed", "rust: 'crates/**'\n")}
	denied := ruleset.deny with input as array.concat(repo, [ci])
	every msg in denied {
		not contains(msg, "merge_group")
	}
}

test_required_check_never_reports if {
	repo := fixtures.without_file(".github/workflows/deps.yml")
	".github/rulesets/protect-main.json: no workflow reports \"Deps passed\", so nothing can merge" in ruleset.deny with input as repo
}
