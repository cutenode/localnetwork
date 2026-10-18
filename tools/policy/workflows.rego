# Pull request workflows follow the changes, work, passed pattern, and every project is in one of
# their path filters (.github/NEW_PROJECT.md, "CI").
package workflows

import data.lib

# Pull request workflows that don't gate merges: benchmarks report to CodSpeed, the release
# workflow is dist's own, which only plans a release on pull requests, and dependency review is
# GitHub's own check of what a pull request adds, which isn't a required check.
ungated := {
	".github/workflows/locallibrary-bench.yml",
	".github/workflows/locallibrary-release.yml",
	".github/workflows/dependency-review.yml",
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
	runs_on_pull_requests(triggers(wf))
}

# `on:` is an event, a list of them, or an object keyed by them (`pull_request:` is often empty).
runs_on_pull_requests(on) if on == "pull_request"

runs_on_pull_requests(on) if "pull_request" in on

runs_on_pull_requests(on) if "pull_request" in object.keys(on)

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
	msg := sprintf("%s: the passed job should exit 1 when a job it needs failed or was cancelled", [path])
}

fails_on_failure(job) if {
	some step in job.steps
	contains(step["if"], "contains(needs.*.result, 'failure')")
	contains(step["if"], "contains(needs.*.result, 'cancelled')")
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
	not wf.concurrency
	msg := sprintf("%s: add a concurrency group that cancels superseded runs", [path])
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
