package workflows_test

import data.fixtures
import data.workflows

ci := fixtures.file(".github/workflows/site-ci.yml")

# The fixture's own report job, in place of the repository's.
reports := {".github/workflows/site-e2e.yml#report"}

with_ci(contents) := fixtures.with_file(".github/workflows/site-ci.yml", contents)

test_fixture_passes if {
	count(workflows.deny) == 0 with input as fixtures.repo with workflows.reports as reports
}

test_gated_workflows if {
	gated := object.keys(workflows.gated) with input as fixtures.repo
	gated == {".github/workflows/site-ci.yml", ".github/workflows/rust.yml", ".github/workflows/deps.yml", ".github/workflows/lint.yml"}
}

test_no_changes_job if {
	broken := json.remove(ci, ["jobs/changes", "jobs/verify/needs", "jobs/passed/needs"])
	".github/workflows/site-ci.yml: add a `changes` job that runs dorny/paths-filter, which the other jobs need (see localtoolbox-ci.yml)" in workflows.deny with input as with_ci(broken)
}

test_unfiltered_workflow_needs_no_changes_job if {
	unfiltered := json.patch(ci, [
		{"op": "remove", "path": "/jobs/changes"},
		{"op": "remove", "path": "/jobs/e2e"},
		{"op": "remove", "path": "/jobs/verify/needs"},
		{"op": "replace", "path": "/jobs/passed/needs", "value": ["verify"]},
	])
	count(workflows.deny) == 0 with input as fixtures.with_file(".github/workflows/pr-issue.yml", unfiltered) with workflows.reports as reports
}

test_trigger_paths if {
	broken := json.patch(ci, [{"op": "replace", "path": "/true/pull_request", "value": {"paths-ignore": ["docs/**"]}}])
	".github/workflows/site-ci.yml: move `on.pull_request.paths-ignore` into the changes job's filter; the passed check has to report on every pull request" in workflows.deny with input as with_ci(broken)
}

test_no_passed_job if {
	broken := json.remove(ci, ["jobs/passed"])
	".github/workflows/site-ci.yml: add a `passed` job that fails when any other job fails (see localtoolbox-ci.yml)" in workflows.deny with input as with_ci(broken)
}

always_msg := ".github/workflows/site-ci.yml: the passed job's `if` should be `always() && (...)`, with the conditions it fails on, so it runs when jobs fail or are skipped"

test_passed_without_if if {
	broken := json.remove(ci, ["jobs/passed/if"])
	always_msg in workflows.deny with input as with_ci(broken)
}

test_passed_always_runs if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/if", "value": "always()"}])
	always_msg in workflows.deny with input as with_ci(broken)
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
	count(workflows.deny) == 0 with input as with_ci(extra) with workflows.reports as reports
}

test_passed_misnamed if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/name", "value": "passed"}])
	".github/workflows/site-ci.yml: name the passed job \"<workflow> passed\"; the ruleset requires it by name" in workflows.deny with input as with_ci(broken)
}

fails_msg := ".github/workflows/site-ci.yml: the passed job should exit 1 when a job it needs failed, was cancelled, or was skipped with work to do"

test_passed_ignores_cancelled_jobs if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/if", "value": "always() && (contains(needs.*.result, 'failure') || needs.changes.outputs.run == 'true' && contains(needs.*.result, 'skipped'))"}])
	fails_msg in workflows.deny with input as with_ci(broken)
}

test_passed_ignores_skipped_jobs if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/if", "value": "always() && (contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled'))"}])
	fails_msg in workflows.deny with input as with_ci(broken)
}

test_passed_never_exits if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/passed/steps", "value": [{"run": "echo failed"}]}])
	fails_msg in workflows.deny with input as with_ci(broken)
}

draft_outputs := {
	"run": "${{ steps.pr.outputs.draft != 'true' && steps.filter.outputs.run == 'true' }}",
	"draft": "${{ steps.pr.outputs.draft == 'true' && steps.filter.outputs.run == 'true' }}",
}

draft_step := {"if": "needs.changes.outputs.draft == 'true'", "run": "exit 1"}

draft_if := "always() && (contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') || (needs.changes.outputs.run == 'true' && contains(needs.*.result, 'skipped')) || needs.changes.outputs.draft == 'true')"

draft_msg := ".github/workflows/site-ci.yml: the passed job should exit 1 when the changes job's draft output is true, or a draft's skipped jobs pass it"

test_draft_from_event if {
	broken := json.patch(ci, [
		{"op": "add", "path": "/jobs/changes/outputs", "value": {"run": "${{ !github.event.pull_request.draft && steps.filter.outputs.run == 'true' }}"}},
		{"op": "add", "path": "/jobs/passed/steps/-", "value": draft_step},
	])
	".github/workflows/site-ci.yml: the changes job's \"run\" output reads draft from the event, which can be stale; read it from the API (see locallibrary-ci.yml)" in workflows.deny with input as with_ci(broken)
}

test_draft_passes if {
	broken := json.patch(ci, [{"op": "add", "path": "/jobs/changes/outputs", "value": draft_outputs}])
	draft_msg in workflows.deny with input as with_ci(broken)
}

# The step never runs when the job's `if` leaves drafts out.
test_draft_step_skipped_with_the_job if {
	broken := json.patch(ci, [
		{"op": "add", "path": "/jobs/changes/outputs", "value": draft_outputs},
		{"op": "add", "path": "/jobs/passed/steps/-", "value": draft_step},
	])
	draft_msg in workflows.deny with input as with_ci(broken)
}

test_draft_fails_passed if {
	drafts := json.patch(ci, [
		{"op": "add", "path": "/jobs/changes/outputs", "value": draft_outputs},
		{"op": "replace", "path": "/jobs/passed/if", "value": draft_if},
		{"op": "add", "path": "/jobs/passed/steps/-", "value": draft_step},
	])
	count(workflows.deny) == 0 with input as with_ci(drafts) with workflows.reports as reports
}

test_job_ignores_changes if {
	broken := json.remove(ci, ["jobs/verify/needs"])
	".github/workflows/site-ci.yml: job \"verify\" should need `changes`, and skip itself when nothing it checks changed" in workflows.deny with input as with_ci(broken)
}

test_no_concurrency if {
	broken := json.remove(ci, ["concurrency"])
	".github/workflows/site-ci.yml: add a concurrency group that cancels superseded runs" in workflows.deny with input as with_ci(broken)
}

test_main_pushes_finish if {
	finishing := json.patch(ci, [{"op": "replace", "path": "/concurrency/cancel-in-progress", "value": "${{ github.event_name != 'push' }}"}])
	count(workflows.deny) == 0 with input as with_ci(finishing) with workflows.reports as reports
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
	count(workflows.deny) == 0 with input as with_ci(pr_only) with workflows.reports as reports
}

e2e := fixtures.file(".github/workflows/site-e2e.yml")

with_e2e(contents) := fixtures.with_file(".github/workflows/site-e2e.yml", contents)

test_called_workflows if {
	called := object.keys(workflows.called) with input as fixtures.repo
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

report_only_msg := ".github/workflows/site-e2e.yml: job \"test\" can't fail; for a report-only check, add it to report_only in tools/policy/workflows.rego with the issue that makes it required"

with_test_step(step) := json.patch(e2e, [{"op": "add", "path": "/jobs/test/steps", "value": [step]}])

test_job_that_cant_fail if {
	broken := json.patch(e2e, [{"op": "add", "path": "/jobs/test/continue-on-error", "value": true}])
	report_only_msg in workflows.deny with input as with_e2e(broken)
}

test_job_that_can_fail_by_expression if {
	broken := json.patch(e2e, [{"op": "add", "path": "/jobs/test/continue-on-error", "value": "${{ matrix.experimental }}"}])
	report_only_msg in workflows.deny with input as with_e2e(broken)
}

test_step_that_cant_fail if {
	broken := with_test_step({"run": "pnpm knip", "continue-on-error": true})
	report_only_msg in workflows.deny with input as with_e2e(broken)
}

test_artifact_download_can_fail if {
	allowed := with_test_step({"uses": "actions/download-artifact@0000000000000000000000000000000000000000", "continue-on-error": true})
	count(workflows.deny) == 0 with input as with_e2e(allowed) with workflows.reports as reports
}

test_report_only_job_with_its_issue if {
	report_only := json.patch(e2e, [{"op": "add", "path": "/jobs/test/continue-on-error", "value": true}])
	count(workflows.deny) == 0 with input as with_e2e(report_only) with workflows.reports as reports
		with workflows.report_only as {".github/workflows/site-e2e.yml#test": "LOC-624"}
}

test_report_job_in_another_workflow if {
	broken := json.patch(ci, [{"op": "add", "path": "/jobs/report", "value": {"needs": "changes", "continue-on-error": true}}])
	".github/workflows/site-ci.yml: job \"report\" can't fail; for a report-only check, add it to report_only in tools/policy/workflows.rego with the issue that makes it required" in workflows.deny with input as with_ci(broken)
		with workflows.reports as reports
}

test_exemption_that_matches_no_job if {
	denied := workflows.deny with input as fixtures.repo
		with workflows.reports as reports | {".github/workflows/gone.yml#report"}
		with workflows.report_only as {".github/workflows/site-e2e.yml#gone": "LOC-624"}
	denied == {
		"tools/policy/workflows.rego: \".github/workflows/gone.yml#report\" in reports matches no job; remove it",
		"tools/policy/workflows.rego: report_only[\".github/workflows/site-e2e.yml#gone\"] matches no job; remove it",
	}
}

test_report_only_without_an_issue if {
	msg := "tools/policy/workflows.rego: report_only[\".github/workflows/site-e2e.yml#test\"] should name the issue that makes it required, as LOC-123"
	msg in workflows.deny with input as fixtures.repo
		with workflows.report_only as {".github/workflows/site-e2e.yml#test": "soon"}
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

# repo_checks takes every file, so a project only it matches still gets none of its own checks.
test_project_only_a_catch_all_filter_has if {
	broken := json.patch(ci, [{"op": "replace", "path": "/jobs/changes/steps/1/with/filters", "value": "run:\n  - 'apps/tool/**'\nrepo_checks:\n  - '**'\n"}])
	"apps/site/package.json: no pull request workflow runs when it changes; add its folder to one's path filter" in workflows.deny with input as with_ci(broken)
}

with_files_filter(statuses) := with_ci(json.patch(ci, [{"op": "replace", "path": "/jobs/changes/steps/1/with/filters", "value": sprintf("run:\n  - 'apps/**'\n  - 'packages/**'\nfiles:\n  - %s: '**'\n", [statuses])}]))

test_added_or_deleted_without_renamed if {
	".github/workflows/site-ci.yml: filter \"files\" matches added|deleted files, so add renamed: a moved file is renamed, not added or deleted" in workflows.deny with input as with_files_filter("added|deleted")
}

test_added_deleted_and_renamed if {
	count(workflows.deny) == 0 with input as with_files_filter("added|deleted|renamed") with workflows.reports as reports
}

test_modified_needs_no_renamed if {
	count(workflows.deny) == 0 with input as with_files_filter("modified") with workflows.reports as reports
}

reads(list) := fixtures.with_file("/scratch/reads.json", {"reads": list})

test_read_outside_the_filter if {
	".github/labels.yml: crates/x/src/lib.rs reads it, so add it to the locallibrary paths filter" in workflows.deny with input as reads([{"from": "crates/x/src/lib.rs", "path": ".github/labels.yml"}])
}

test_read_the_filter_excludes if {
	"crates/y/README.md: apps/locallibrary/src/x.rs reads it, so add it to the locallibrary paths filter" in workflows.deny with input as reads([{"from": "apps/locallibrary/src/x.rs", "path": "crates/y/README.md"}])
}

test_read_of_a_folder_outside_the_filter if {
	"docs/: crates/x/tests/a.rs reads it, so add it to the locallibrary paths filter" in workflows.deny with input as reads([{"from": "crates/x/tests/a.rs", "path": "docs/"}])
}

# Bombadil is in apps/locallibrary/, but only a weekly run reads what its specs read.
test_read_from_a_folder_no_pull_request_runs if {
	count(workflows.deny) == 0 with input as reads([{"from": "apps/locallibrary/bombadil/lib.mjs", "path": "docs/a.pdb"}]) with workflows.reports as reports
}

test_read_from_the_longest_folder if {
	"docs/a.pdb: apps/locallibrary/bombadil/lib.mjs reads it, so add it to the locallibrary paths filter" in workflows.deny with input as reads([{"from": "apps/locallibrary/bombadil/lib.mjs", "path": "docs/a.pdb"}])
		with workflows.readers as {"apps/": set(), "apps/locallibrary/bombadil/": {"locallibrary"}}
}

test_read_any_of_several_filters_can_have if {
	"crates/x/README.md: tools/a.test.ts reads it, so add it to the repo_checks or repo_checks_md paths filter" in workflows.deny with input as reads([{"from": "tools/a.test.ts", "path": "crates/x/README.md"}])
}

test_read_from_a_folder_with_no_filters if {
	"scripts/a.ts: it reads crates/x/src/lib.rs, so add its folder to workflows.rego's readers with the filters that run its checks" in workflows.deny with input as reads([{"from": "scripts/a.ts", "path": "crates/x/src/lib.rs"}])
}

with_codeql_filter(globs) := with_ci(json.patch(ci, [{"op": "replace", "path": "/jobs/changes/steps/1/with/filters", "value": sprintf("run:\n  - 'apps/**'\n  - 'packages/**'\ncodeql_rust:\n%s", [concat("", [sprintf("  - '%s'\n", [g]) | some g in globs])])}]))

test_codeql_filter_without_its_workflow if {
	".github/workflows/codeql.yml: add it to the codeql_rust paths filter; CodeQL's results change with it" in workflows.deny with input as with_codeql_filter(["**/*.rs", ".github/codeql/**"])
}

test_codeql_filter_without_its_config if {
	".github/codeql/codeql-config.yml: add it to the codeql_rust paths filter; CodeQL's results change with it" in workflows.deny with input as with_codeql_filter(["**/*.rs", ".github/workflows/codeql.yml"])
}

test_codeql_filter_with_both if {
	denied := workflows.deny with input as with_codeql_filter(["**/*.rs", ".github/codeql/**", ".github/workflows/codeql.yml"])
	every msg in denied {
		not contains(msg, "codeql_rust")
	}
}

budgets(base) := fixtures.with_file(".github/workflows/site-e2e.yml", json.patch(fixtures.file(".github/workflows/site-e2e.yml"), [{"op": "add", "path": "/jobs/test/steps", "value": [{"run": "check", "env": {"GROUP_BASE": base}}]}]))

test_budgets_over_the_entry_ahead if {
	".github/workflows/site-e2e.yml: job \"test\" reads the entry ahead of a merge queue group (base_sha); take main's tip from the workflow that calls it, as `inputs.group-base`" in workflows.deny with input as budgets("${{ github.event.merge_group.base_sha }}")
}

test_budgets_over_main if {
	count(workflows.deny) == 0 with input as budgets("${{ inputs.group-base }}") with workflows.reports as reports
}

# A changes job that lets a group skip Windows, with the step's env and the globs of workflow_lint.
with_windows_skip(env, globs) := with_ci(json.patch(ci, [
	{"op": "replace", "path": "/jobs/changes/steps/1/with/filters", "value": sprintf("run:\n  - 'apps/**'\n  - 'packages/**'\nworkflow_lint:\n%s", [concat("", [sprintf("  - '%s'\n", [g]) | some g in globs])])},
	{"op": "add", "path": "/jobs/changes/steps/-", "value": {"run": "node tools/queue/cli.ts >> \"$GITHUB_OUTPUT\"", "env": env}},
]))

from_filter := {"WORKFLOWS_CHANGED": "${{ steps.filter.outputs.workflow_lint }}"}

test_windows_skip_without_the_filter if {
	".github/workflows/site-ci.yml: give the tools/queue/cli.ts step `WORKFLOWS_CHANGED: ${{ steps.filter.outputs.workflow_lint }}`, so a group that changes a workflow runs Windows" in workflows.deny
		with input as with_windows_skip({"WORKFLOWS_CHANGED": "false"}, [".github/workflows/**", ".github/actions/**"])
}

test_windows_skip_without_the_actions if {
	".github/actions/last-passed/action.yml: add it to the workflow_lint paths filter; a merge queue group that changes it has to run Windows" in workflows.deny with input as with_windows_skip(from_filter, [".github/workflows/**"])
}

test_windows_skip_without_the_workflows if {
	".github/workflows/ci.yml: add it to the workflow_lint paths filter; a merge queue group that changes it has to run Windows" in workflows.deny with input as with_windows_skip(from_filter, [".github/actions/**"])
}

test_windows_skip if {
	count(workflows.deny) == 0 with input as with_windows_skip(from_filter, [".github/workflows/**", ".github/actions/**"])
		with workflows.reports as reports
}

install(tools) := {"true": ["workflow_dispatch"], "jobs": {"lint": {"steps": [
	{"uses": "taiki-e/install-action@0000000000000000000000000000000000000000", "with": {"tool": tools}},
]}}}

test_install_without_version if {
	repo := fixtures.with_file(".github/workflows/tools.yml", install("cargo-deny@0.20.2, zizmor"))
	".github/workflows/tools.yml: job \"lint\" installs zizmor with no exact version; name one, as zizmor@1.2.3" in workflows.deny with input as repo
}

test_install_with_versions if {
	denied := workflows.deny with input as fixtures.with_file(".github/workflows/tools.yml", install("cargo-nextest@0.9.148,cargo-llvm-cov@0.9.1"))
	every msg in denied {
		not contains(msg, "tools.yml")
	}
}

apt(packages) := {"runs": {"using": "composite", "steps": [{"uses": "./.github/actions/apt-packages", "with": {"packages": packages}}]}}

test_apt_packages_from_an_expression if {
	repo := fixtures.with_file(".github/actions/build/action.yml", apt("${{ inputs.packages }}"))
	".github/actions/build/action.yml: name the apt packages \"${{ inputs.packages }}\" stands for; locallibrary-cache.yml can't warm an expression" in workflows.deny with input as repo
}

test_apt_packages_from_an_expression_in_a_workflow if {
	wf := {"true": ["workflow_dispatch"], "jobs": {"build": {"steps": apt("${{ matrix.packages }}").runs.steps}}}
	repo := fixtures.with_file(".github/workflows/build.yml", wf)
	".github/workflows/build.yml: name the apt packages \"${{ matrix.packages }}\" stands for; locallibrary-cache.yml can't warm an expression" in workflows.deny with input as repo
	warmer := workflows.deny with input as fixtures.with_file(".github/workflows/locallibrary-cache.yml", wf)
	every msg in warmer {
		not contains(msg, "locallibrary-cache.yml: name the apt packages")
	}
}

test_apt_packages_named if {
	denied := workflows.deny with input as fixtures.with_file(".github/actions/build/action.yml", apt("libasound2-dev valgrind"))
	every msg in denied {
		not contains(msg, "build/action.yml")
	}
}
