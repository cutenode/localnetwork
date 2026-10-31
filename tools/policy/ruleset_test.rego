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

scanning := {"type": "code_scanning", "parameters": {"code_scanning_tools": [{"tool": "CodeQL"}]}}

codeql(on) := {"path": ".github/workflows/codeql.yml", "contents": {"true": on, "jobs": {"analyze": {}}}}

with_queue_and_scanning(on) := array.concat(
	fixtures.with_file(".github/rulesets/protect-main.json", json.patch(fixtures.file(".github/rulesets/protect-main.json"), [
		{"op": "add", "path": "/rules/-", "value": queue},
		{"op": "add", "path": "/rules/-", "value": scanning},
	])),
	[codeql(on)],
)

codeql_msg := ".github/workflows/codeql.yml: run it on merge_group, or call it from a workflow that does (as ci.yml does); the code scanning rule holds each merge queue group until CodeQL analyses its commit"

test_merge_queue_with_codeql_off_merge_groups if {
	codeql_msg in ruleset.deny with input as with_queue_and_scanning({"pull_request": null})
}

test_merge_queue_with_codeql_on_merge_groups if {
	not codeql_msg in ruleset.deny with input as with_queue_and_scanning({"pull_request": null, "merge_group": null})
}

caller(on) := {"path": ".github/workflows/ci.yml", "contents": {"true": on, "jobs": {"codeql": {"uses": "./.github/workflows/codeql.yml"}}}}

test_merge_queue_with_codeql_called_on_merge_groups if {
	repo := array.concat(with_queue_and_scanning({"workflow_call": null}), [caller({"pull_request": null, "merge_group": null})])
	not codeql_msg in ruleset.deny with input as repo
}

test_merge_queue_with_codeql_called_off_merge_groups if {
	repo := array.concat(with_queue_and_scanning({"workflow_call": null}), [caller({"pull_request": null})])
	codeql_msg in ruleset.deny with input as repo
}

test_required_check_never_reports if {
	repo := fixtures.without_file(".github/workflows/deps.yml")
	".github/rulesets/protect-main.json: no workflow reports \"Deps passed\", so nothing can merge" in ruleset.deny with input as repo
}
