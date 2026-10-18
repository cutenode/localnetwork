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

test_required_check_never_reports if {
	repo := fixtures.without_file(".github/workflows/deps.yml")
	".github/rulesets/protect-main.json: no workflow reports \"Deps passed\", so nothing can merge" in ruleset.deny with input as repo
}
