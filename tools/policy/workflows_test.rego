package workflows_test

import data.fixtures
import data.workflows

ci := fixtures.file(".github/workflows/site-ci.yml")

with_ci(contents) := fixtures.with_file(".github/workflows/site-ci.yml", contents)

test_fixture_passes if {
	count(workflows.deny) == 0 with input as fixtures.repo
}

test_gated_workflows if {
	gated := {path | some path, _ in workflows.gated} with input as fixtures.repo
	gated == {".github/workflows/site-ci.yml", ".github/workflows/rust.yml", ".github/workflows/deps.yml", ".github/workflows/lint.yml"}
}

test_ungated_workflow_is_skipped if {
	broken := json.remove(ci, ["jobs/passed"])
	repo := fixtures.with_file(".github/workflows/locallibrary-bench.yml", broken)
	count(workflows.deny) == 0 with input as repo
}

test_no_changes_job if {
	broken := json.remove(ci, ["jobs/changes", "jobs/verify/needs", "jobs/passed/needs"])
	".github/workflows/site-ci.yml: add a `changes` job that runs dorny/paths-filter, which the other jobs need (see localtoolbox-ci.yml)" in workflows.deny with input as with_ci(broken)
}

test_trigger_paths if {
	broken := json.patch(ci, [{"op": "replace", "path": "/true/pull_request", "value": {"paths-ignore": ["docs/**"]}}])
	".github/workflows/site-ci.yml: move `on.pull_request.paths-ignore` into the changes job's filter; the passed check has to report on every pull request" in workflows.deny with input as with_ci(broken)
}

test_no_passed_job if {
	broken := json.remove(ci, ["jobs/passed"])
	".github/workflows/site-ci.yml: add a `passed` job that fails when any other job fails (see localtoolbox-ci.yml)" in workflows.deny with input as with_ci(broken)
}

test_passed_without_always if {
	broken := json.remove(ci, ["jobs/passed/if"])
	".github/workflows/site-ci.yml: the passed job needs `if: always()`, so it reports when jobs are skipped" in workflows.deny with input as with_ci(broken)
}

test_passed_misses_a_job if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/needs", "value": "changes"}])
	".github/workflows/site-ci.yml: the passed job should need \"verify\"" in workflows.deny with input as with_ci(broken)
}

test_report_and_semver_stay_outside_passed if {
	extra := json.patch(ci, [
		{"op": "add", "path": "/jobs/report", "value": {"needs": ["changes"], "runs-on": "ubuntu-latest"}},
		{"op": "add", "path": "/jobs/semver", "value": {"needs": "changes", "runs-on": "ubuntu-latest"}},
	])
	count(workflows.deny) == 0 with input as with_ci(extra)
}

test_passed_misnamed if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/name", "value": "passed"}])
	".github/workflows/site-ci.yml: name the passed job \"<workflow> passed\"; the ruleset requires it by name" in workflows.deny with input as with_ci(broken)
}

test_passed_never_fails if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/steps", "value": [{"if": "contains(needs.*.result, 'failure')", "run": "exit 1"}]}])
	".github/workflows/site-ci.yml: the passed job should exit 1 when a job it needs failed or was cancelled" in workflows.deny with input as with_ci(broken)
}

test_job_ignores_changes if {
	broken := json.remove(ci, ["jobs/verify/needs"])
	".github/workflows/site-ci.yml: job \"verify\" should need `changes`, and skip itself when nothing it checks changed" in workflows.deny with input as with_ci(broken)
}

test_no_concurrency if {
	broken := json.remove(ci, ["concurrency"])
	".github/workflows/site-ci.yml: add a concurrency group that cancels superseded runs" in workflows.deny with input as with_ci(broken)
}

test_project_outside_every_filter if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/changes/steps/0/with/filters", "value": "run:\n  - 'apps/tool/**'\n"}])
	denied := workflows.deny with input as with_ci(broken)
	"apps/site/package.json: no pull request workflow runs when it changes; add its folder to one's path filter" in denied
	"packages/kit/package.json: no pull request workflow runs when it changes; add its folder to one's path filter" in denied
}
