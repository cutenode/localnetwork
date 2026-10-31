# Pull request workflows follow the changes, work, passed pattern, and every project is in one of
# their path filters (.github/NEW_PROJECT.md, "CI").
package workflows

import data.lib

# Pull request workflows with no passed check: benchmarks report to CodSpeed, the release workflow
# is dist's own, which only plans a release on pull requests, dependency review is GitHub's own
# check of what a pull request adds, which isn't a required check, and CodeQL holds merges through
# the ruleset's code scanning rule, which reads its alerts.
ungated := {
	".github/workflows/locallibrary-bench.yml",
	".github/workflows/locallibrary-release.yml",
	".github/workflows/dependency-review.yml",
	".github/workflows/codeql.yml",
}

# Jobs that stay out of the passed check's needs on purpose: `report` posts coverage on the pull
# request, so a comment that can't post never holds up a merge, and `semver` only reports breaking
# changes in the loclib-* crates, which aren't published yet.
outside_passed := {"report", "semver"}

# YAML 1.1 reads the `on:` key as true, which is how conftest parses it.
triggers(wf) := wf["true"]

# Workflows that run on pull requests and have to report a "... passed" check.
gated[path] := wf if {
	some path in lib.paths(".github/workflows/*.yml")
	not path in ungated
	wf := lib.file(path)
	runs_on(triggers(wf), "pull_request")
}

# `on:` is an event, a list of them, or an object keyed by them (`pull_request:` is often empty).
runs_on(on, event) if on == event

runs_on(on, event) if event in on

runs_on(on, event) if event in object.keys(on)

# The jobs a job waits for, as a set.
needs(job) := lib.strings(object.get(job, "needs", []))

deny contains msg if {
	some path, wf in gated
	not wf.jobs.changes
	msg := sprintf("%s: add a `changes` job that runs dorny/paths-filter, which the other jobs need (see localtoolbox-ci.yml)", [path])
}

# A required check that never starts blocks every pull request, so the paths a workflow checks go
# in its changes job, never in its trigger.
deny contains msg if {
	some path, wf in gated
	some key in ["paths", "paths-ignore"]
	object.get(object.get(triggers(wf), "pull_request", {}), key, null) != null
	msg := sprintf("%s: move `on.pull_request.%s` into the changes job's filter; the passed check has to report on every pull request", [path, key])
}

deny contains msg if {
	some path, wf in gated
	not wf.jobs.passed
	msg := sprintf("%s: add a `passed` job that fails when any other job fails (see localtoolbox-ci.yml)", [path])
}

deny contains msg if {
	some path, wf in gated
	passed := wf.jobs.passed
	object.get(passed, "if", "") != "always()"
	msg := sprintf("%s: the passed job needs `if: always()`, so it reports when jobs are skipped", [path])
}

deny contains msg if {
	some path, wf in gated
	passed := wf.jobs.passed
	others := {id | some id, _ in wf.jobs; id != "passed"}
	some id in ((others - needs(passed)) - outside_passed)
	msg := sprintf("%s: the passed job should need %q", [path, id])
}

deny contains msg if {
	some path, wf in gated
	passed := wf.jobs.passed
	not endswith(object.get(passed, "name", ""), " passed")
	msg := sprintf("%s: name the passed job \"<workflow> passed\"; the ruleset requires it by name", [path])
}

deny contains msg if {
	some path, wf in gated
	passed := wf.jobs.passed
	not fails_on_failure(passed)
	msg := sprintf("%s: the passed job should exit 1 when a job it needs failed, was cancelled, or was skipped with work to do", [path])
}

# A job skipped because one it needs didn't finish reports `skipped`, as one with nothing to check
# does, so the step also checks for skipped jobs next to the changes job's outputs: #88's tests
# were skipped after its macOS build was cancelled, and locallibrary CI passed.
fails_on_failure(job) if {
	some step in job.steps
	contains(step["if"], "contains(needs.*.result, 'failure')")
	contains(step["if"], "contains(needs.*.result, 'cancelled')")
	contains(step["if"], "'skipped'")
}

# A draft's run skips the work, and skipped jobs pass the passed check, so a draft marked ready
# that got no run after it could merge untested, as #211 did. Its push's event still said draft
# (and a re-run gets the same event), so the changes job reads the draft state from the API, and
# the passed job fails on the draft output.
deny contains msg if {
	some path, wf in gated
	some name, value in object.get(wf.jobs.changes, "outputs", {})
	contains(value, "github.event.pull_request.draft")
	msg := sprintf("%s: the changes job's %q output reads draft from the event, which can be stale; read it from the API (see locallibrary-ci.yml)", [path, name])
}

deny contains msg if {
	some path, wf in gated
	some _, value in object.get(wf.jobs.changes, "outputs", {})
	contains(value, "draft")
	not fails_on_draft(wf.jobs.passed)
	msg := sprintf("%s: the passed job should exit 1 when the changes job's draft output is true, or a draft's skipped jobs pass it", [path])
}

fails_on_draft(job) if {
	some step in job.steps
	step["if"] == "needs.changes.outputs.draft == 'true'"
}

deny contains msg if {
	some path, wf in gated
	wf.jobs.changes
	some id, job in wf.jobs
	not id in {"changes", "passed"}
	not "changes" in needs(job)
	msg := sprintf("%s: job %q should need `changes`, and skip itself when nothing it checks changed", [path, id])
}

deny contains msg if {
	some path, wf in gated
	not cancels_superseded(wf)
	msg := sprintf("%s: add a concurrency group that cancels superseded runs", [path])
}

# On every event: on main too, the next run's changes job covers what a cancelled run didn't check
# (see the rule below).
cancels_superseded(wf) if wf.concurrency["cancel-in-progress"] == true

# A push diffed against the commit before it drops what a cancelled or failed run never checked
# whenever the next push touches other files, so it's diffed against the last commit the workflow
# passed on, from an earlier .github/actions/last-passed step.
deny contains msg if {
	some path, wf in gated
	runs_on(triggers(wf), "push")
	some id, job in wf.jobs
	some i, step in job.steps
	startswith(step.uses, "dorny/paths-filter@")
	not base_from_last_passed(job, i)
	msg := sprintf("%s: job %q's paths-filter needs `base` from an earlier ./.github/actions/last-passed step", [path, id])
}

base_from_last_passed(job, filter) if {
	some i, step in job.steps
	i < filter
	step.uses == "./.github/actions/last-passed"
	job.steps[filter]["with"].base == sprintf("${{ steps.%s.outputs.sha }}", [step.id])
}

# Every glob a gated workflow's changes job filters on.
filter_globs contains glob if {
	some _, wf in gated
	some _, job in wf.jobs
	some step in job.steps
	startswith(step.uses, "dorny/paths-filter@")
	some _, globs in yaml.unmarshal(step["with"].filters)
	some glob in lib.strings(globs)
	not startswith(glob, "!")
}

deny contains msg if {
	some manifest in lib.manifests
	not covered(manifest)
	msg := sprintf("%s: no pull request workflow runs when it changes; add its folder to one's path filter", [manifest])
}

covered(path) if {
	some glob in filter_globs
	lib.matches(glob, path)
}
