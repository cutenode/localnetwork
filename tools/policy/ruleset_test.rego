package ruleset_test

import data.fixtures
import data.ruleset

test_fixture_passes if {
	count(ruleset.deny) == 0 with input as fixtures.repo
}

test_check_not_required if {
	broken := {"rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "Site CI passed"}, {"context": "Rust passed"}, {"context": "Lint passed"}]}}]}
	".github/rulesets/protect-main.json: require \"Deps passed\", then an admin applies it to GitHub's copy" in ruleset.deny with input as fixtures.with_file(".github/rulesets/protect-main.json", broken)
}

review_rule(parameters) := fixtures.with_file(
	".github/rulesets/protect-main.json",
	{"rules": [{"type": "pull_request", "parameters": parameters}]},
)

review_denial(setting) := sprintf(
	".github/rulesets/protect-main.json: set the pull_request rule's %s to true, so an approval doesn't carry over a later push",
	[setting],
)

test_stale_approvals_kept if {
	denied := ruleset.deny with input as review_rule({"dismiss_stale_reviews_on_push": false, "require_last_push_approval": true})
	review_denial("dismiss_stale_reviews_on_push") in denied
}

test_last_push_approval_missing if {
	review_denial("require_last_push_approval") in ruleset.deny with input as review_rule({"dismiss_stale_reviews_on_push": true})
}

pinned(checks) := fixtures.with_file(".github/rulesets/protect-main.json", {"rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": array.concat(
	[{"context": name, "integration_id": 15368} | some name in ["Site CI passed", "Rust passed", "Deps passed"]],
	checks,
)}}]})

test_check_any_app_can_pass if {
	".github/rulesets/protect-main.json: give \"Lint passed\" `\"integration_id\": 15368`, so only GitHub Actions can pass it" in ruleset.deny with input as pinned([{"context": "Lint passed"}])
}

test_check_another_app_passes if {
	".github/rulesets/protect-main.json: give \"Lint passed\" `\"integration_id\": 15368`, so only GitHub Actions can pass it" in ruleset.deny with input as pinned([{"context": "Lint passed", "integration_id": 1}])
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

every_language := {"strategy": {"matrix": {"language": ["actions", "rust"]}}}

codeql(on) := {"path": ".github/workflows/codeql.yml", "contents": {"true": on, "jobs": {"analyze": every_language}}}

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

languages_msg := ".github/workflows/codeql.yml: list every language in job \"analyze\"'s matrix; the code scanning rule holds a commit until each category main has gets a result"

test_codeql_with_every_language if {
	not languages_msg in ruleset.deny with input as with_queue_and_scanning({"workflow_call": null})
}

test_codeql_with_languages_that_changed if {
	some_languages := {"path": ".github/workflows/codeql.yml", "contents": {"true": {"workflow_call": null}, "jobs": {"analyze": {"strategy": {"matrix": {"language": "${{ fromJSON(inputs.languages) }}"}}}}}}
	repo := array.concat([f | some f in with_queue_and_scanning({"workflow_call": null}); f.path != ".github/workflows/codeql.yml"], [some_languages])
	languages_msg in ruleset.deny with input as repo
}
