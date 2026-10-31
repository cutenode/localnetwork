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
	".github/workflows/site-ci.yml: the passed job should exit 1 when a job it needs failed, was cancelled, or was skipped with work to do" in workflows.deny with input as with_ci(broken)
}

test_passed_ignores_skipped_jobs if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/steps", "value": [{"if": "contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled')", "run": "exit 1"}]}])
	".github/workflows/site-ci.yml: the passed job should exit 1 when a job it needs failed, was cancelled, or was skipped with work to do" in workflows.deny with input as with_ci(broken)
}

draft_outputs := {
	"run": "${{ steps.pr.outputs.draft != 'true' && steps.filter.outputs.run == 'true' }}",
	"draft": "${{ steps.pr.outputs.draft == 'true' && steps.filter.outputs.run == 'true' }}",
}

draft_step := {"if": "needs.changes.outputs.draft == 'true'", "run": "exit 1"}

test_draft_from_event if {
	broken := json.patch(ci, [
		{"op": "add", "path": "/jobs/changes/outputs", "value": {"run": "${{ !github.event.pull_request.draft && steps.filter.outputs.run == 'true' }}"}},
		{"op": "add", "path": "/jobs/passed/steps/-", "value": draft_step},
	])
	".github/workflows/site-ci.yml: the changes job's \"run\" output reads draft from the event, which can be stale; read it from the API (see locallibrary-ci.yml)" in workflows.deny with input as with_ci(broken)
}

test_draft_passes if {
	broken := json.patch(ci, [{"op": "add", "path": "/jobs/changes/outputs", "value": draft_outputs}])
	".github/workflows/site-ci.yml: the passed job should exit 1 when the changes job's draft output is true, or a draft's skipped jobs pass it" in workflows.deny with input as with_ci(broken)
}

test_draft_fails_passed if {
	drafts := json.patch(ci, [
		{"op": "add", "path": "/jobs/changes/outputs", "value": draft_outputs},
		{"op": "add", "path": "/jobs/passed/steps/-", "value": draft_step},
	])
	count(workflows.deny) == 0 with input as with_ci(drafts)
}

test_job_ignores_changes if {
	broken := json.remove(ci, ["jobs/verify/needs"])
	".github/workflows/site-ci.yml: job \"verify\" should need `changes`, and skip itself when nothing it checks changed" in workflows.deny with input as with_ci(broken)
}

test_no_concurrency if {
	broken := json.remove(ci, ["concurrency"])
	".github/workflows/site-ci.yml: add a concurrency group that cancels superseded runs" in workflows.deny with input as with_ci(broken)
}

test_main_runs_not_cancelled if {
	broken := json.patch(ci, [{"op": "replace", "path": "/concurrency/cancel-in-progress", "value": "${{ github.event_name == 'pull_request' }}"}])
	".github/workflows/site-ci.yml: add a concurrency group that cancels superseded runs" in workflows.deny with input as with_ci(broken)
}

base_msg := ".github/workflows/site-ci.yml: job \"changes\"'s paths-filter needs `base` from an earlier ./.github/actions/last-passed step"

test_push_diffed_against_commit_before if {
	broken := json.remove(ci, ["jobs/changes/steps/1/with/base"])
	base_msg in workflows.deny with input as with_ci(broken)
}

test_last_passed_after_filter if {
	steps := ci.jobs.changes.steps
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/changes/steps", "value": [steps[1], steps[0]]}])
	base_msg in workflows.deny with input as with_ci(broken)
}

test_pull_request_only_needs_no_base if {
	pr_only := json.patch(ci, [
		{"op": "replace", "path": "/true", "value": "pull_request"},
		{"op": "remove", "path": "/jobs/changes/steps/1/with/base"},
	])
	count(workflows.deny) == 0 with input as with_ci(pr_only)
}

e2e := fixtures.file(".github/workflows/site-e2e.yml")

with_e2e(contents) := fixtures.with_file(".github/workflows/site-e2e.yml", contents)

test_called_workflows if {
	called := {path | some path, _ in workflows.called} with input as fixtures.repo
	called == {".github/workflows/site-e2e.yml"}
}

test_called_workflow_with_concurrency if {
	broken := json.patch(e2e, [{"op": "add", "path": "/concurrency", "value": {"group": "${{ github.workflow }}", "cancel-in-progress": true}}])
	".github/workflows/site-e2e.yml: remove its concurrency group; a call gets the calling workflow's, so it would cancel the run that called it" in workflows.deny with input as with_e2e(broken)
}

test_called_workflow_on_push if {
	broken := json.patch(e2e, [{"op": "add", "path": "/true/push", "value": {"branches": ["main"]}}])
	".github/workflows/site-e2e.yml: remove `on.push`; the workflow that calls it already runs it then" in workflows.deny with input as with_e2e(broken)
}

test_called_report_can_fail_the_call if {
	broken := json.remove(e2e, ["jobs/report/continue-on-error"])
	".github/workflows/site-e2e.yml: give job \"report\" `continue-on-error: true`, or it fails the call when it fails" in workflows.deny with input as with_e2e(broken)
}

test_call_without_a_permission_its_jobs_ask_for if {
	broken := json.remove(ci, ["jobs/e2e/permissions"])
	".github/workflows/site-ci.yml: job \"e2e\" should grant `pull-requests: write`, which .github/workflows/site-e2e.yml's \"report\" job asks for" in workflows.deny with input as with_ci(broken)
}

test_call_granting_read_where_write_is_asked if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/e2e/permissions/pull-requests", "value": "read"}])
	".github/workflows/site-ci.yml: job \"e2e\" should grant `pull-requests: write`, which .github/workflows/site-e2e.yml's \"report\" job asks for" in workflows.deny with input as with_ci(broken)
}

test_project_outside_every_filter if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/changes/steps/1/with/filters", "value": "run:\n  - 'apps/tool/**'\n"}])
	denied := workflows.deny with input as with_ci(broken)
	"apps/site/package.json: no pull request workflow runs when it changes; add its folder to one's path filter" in denied
	"packages/kit/package.json: no pull request workflow runs when it changes; add its folder to one's path filter" in denied
}
