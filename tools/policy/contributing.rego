# .github/CONTRIBUTING.md gives every check CI runs on a pull request: each step of the workflows
# that run on pull requests, and of the ones they call, is a command in one of its code blocks or a
# step of a verify script one of them names (tools/verify/steps.json), unless `unlisted` says why
# not.
package contributing

import data.apps
import data.lib
import data.workflows

# The lines of CONTRIBUTING's bash code blocks, which tools/policy/inputs.ts writes as
# {"commands": [...]} in contributing.json.
commands := f.contents.commands if {
	some f in input
	regex.match(`(^|/)contributing\.json$`, f.path)
}

# Jobs ("<workflow>#<job>") and steps (by name, command or action) with nothing for a contributor to
# run, and why.
unlisted := {
	".github/workflows/ci.yml#changes": "it picks the jobs a change needs",
	".github/workflows/ci.yml#passed": "the check the ruleset requires, which fails when a job above did",
	".github/workflows/pr-issue.yml#issue": "it reads the pull request's description, which Pull requests covers",
	".github/workflows/pr-issue.yml#passed": "the check the ruleset requires, which fails when a job above did",
	".github/workflows/locallibrary-ci.yml#other-os": "verify:rust's clippy, doctests and tests, on macOS and Windows",
	".github/workflows/locallibrary-ci.yml#fuzz": "each fuzz target for a minute, which apps/locallibrary/CONTRIBUTING.md covers",
	".github/workflows/locallibrary-ci.yml#budgets": "it measures on CI's runners against main's numbers (Gates)",
	".github/workflows/locallibrary-ci.yml#release-budgets": "it measures on CI's runners against main's numbers (Gates)",
	".github/workflows/dependency-review.yml#review": "GitHub's review of the dependencies a pull request adds",
	".github/workflows/codeql.yml#analyze": "GitHub's code scanning",
	"actions/upload-artifact": "it hands results to a later job",
	"actions/download-artifact": "it takes results from an earlier job",
	"./.github/actions/patch-coverage": "it covers the lines a pull request changes (Gates)",
	"pnpm exec playwright install chromium": "Setup installs it",
	"rustup target add x86_64-pc-windows-gnu": "Setup installs it",
	"rustup toolchain install 1.91 --profile minimal": "Setup installs it",
	"go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.12": "Setup installs it",
	"Install StyLua and selene": "Setup installs them",
	"Pull the HappyView and Postgres images (scripts/lib/happyview.ts)": "the tests pull them as they start",
	"sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0": "Ubuntu's runners need it for Chrome's sandbox",
	"docker/setup-buildx-action": "it sets up the image build test:docker runs against",
	"docker/build-push-action": "it builds the image test:docker runs against",
	"Scores to the run's summary": "it reports Lighthouse's scores",
	"Measure the client bundle": "it measures the size the report compares with main",
	"pnpm zip": "it builds the zip whose size the report compares with main",
	"Measure the package": "it measures the size the report compares with main",
}

# The budgets' steps in a job that checks more, which measure on CI's runners and compare with
# main's numbers (Gates).
budgets(step) if {
	some marker in {"tools/budgets/", "bundle-sizes.ts", "Budget-Raise"}
	contains(object.get(step, "run", ""), marker)
}

pull_request_workflows := object.union(workflows.gated, workflows.called)

# A command CONTRIBUTING gives, alone on its line or with more after it.
named(command) if {
	some line in commands
	line == command
}

named(command) if {
	some line in commands
	startswith(line, concat("", [command, " "]))
}

# What the verify scripts CONTRIBUTING names run, as [folder, command]: the root's steps, and
# through `pnpm -r --no-bail verify`, each app's and package's own `verify` in its folder.
verified contains ["", command] if {
	some script, steps in lib.file("tools/verify/steps.json")
	named(concat(" ", ["pnpm", script]))
	some command in steps
}

verified contains [trim_suffix(path, "/package.json"), trim_space(command)] if {
	some script, steps in lib.file("tools/verify/steps.json")
	named(concat(" ", ["pnpm", script]))
	"pnpm -r --no-bail verify" in steps
	some path in (lib.paths("apps/*/package.json") | lib.paths("packages/*/package.json"))
	some command in split(lib.file(path).scripts.verify, "&&")
}

# The folder a step runs in, from the repository root ("" for the root itself).
folder(wf, job, step) := trim(dir, ".") if {
	dir := object.get(step, "working-directory", object.get(job, ["defaults", "run", "working-directory"], object.get(wf, ["defaults", "run", "working-directory"], "")))
}

# How CONTRIBUTING gives a step's key: its spelling, or the command run in its folder from the root.
spellings(key, _) := {apps.spelling[key]}

spellings(key, "") := {key} if not apps.spelling[key]

spellings(key, dir) := {sprintf("pnpm -C %s %s", [dir, trim_prefix(key, "pnpm ")]), sprintf("(cd %s && %s)", [dir, key])} if {
	dir != ""
	not apps.spelling[key]
}

covered(keys, dir) if {
	some key in keys
	["", apps.spelled(key, dir)] in verified
}

covered(keys, dir) if {
	some key in keys
	[dir, key] in verified
}

covered(keys, dir) if {
	some key in keys
	some command in spellings(key, dir)
	named(command)
}

deny contains msg if {
	some path, wf in pull_request_workflows
	some id, job in wf.jobs
	not id in workflows.outside_passed
	some step in object.get(job, "steps", [])
	keys := apps.step_keys(step) | {sprintf("%s#%s", [path, id])}
	count(keys & (apps.setup | object.keys(unlisted))) == 0
	not budgets(step)
	not covered(keys, folder(wf, job, step))
	msg := sprintf("%s: give %v (job %s) in .github/CONTRIBUTING.md, or add it to unlisted in tools/policy/contributing.rego with why", [path, sort(apps.step_keys(step)), id])
}

# An entry that matches nothing hides the next step that takes its name.
deny contains msg if {
	some key in object.keys(unlisted)
	not listed_somewhere(key)
	msg := sprintf("tools/policy/contributing.rego: unlisted[%q] matches no job or step of a pull request's workflows; remove it", [key])
}

listed_somewhere(key) if {
	some path, wf in pull_request_workflows
	some id, _ in wf.jobs
	key == sprintf("%s#%s", [path, id])
}

listed_somewhere(key) if {
	some wf in pull_request_workflows
	some job in wf.jobs
	some step in object.get(job, "steps", [])
	key in apps.step_keys(step)
}
