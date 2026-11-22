# Each TypeScript app has what the root's scripts, CI and the git hooks expect of it
# (.github/NEW_PROJECT.md, "A new TypeScript app").
package apps

import data.lib
import data.workflows

# The scripts the root's `pnpm -r` scripts and the CI workflows call.
scripts := {"dev", "build", "check", "lint", "format", "format:check", "test", "test:coverage", "verify"}

# The checks `verify` has to run, as CI's verify jobs do.
verify_steps := {"pnpm format:check", "pnpm lint", "pnpm check"}

root := lib.file("package.json")

package_json(app) := lib.file(sprintf("apps/%s/package.json", [app]))

deny contains msg if {
	some app in lib.ts_apps
	some script in scripts
	not package_json(app).scripts[script]
	msg := sprintf("apps/%s/package.json: add a %q script", [app, script])
}

deny contains msg if {
	some app in lib.ts_apps
	commands := {trim_space(c) | some c in split(package_json(app).scripts.verify, "&&")}
	some step in (verify_steps - commands)
	msg := sprintf("apps/%s/package.json: `verify` should run `%s`", [app, step])
}

deny contains msg if {
	some app in lib.ts_apps
	commands := {trim_space(c) | some c in split(package_json(app).scripts.verify, "&&")}
	not any_test(commands)
	msg := sprintf("apps/%s/package.json: `verify` should run its tests", [app])
}

any_test(commands) if {
	some c in commands
	regex.match(`^pnpm test(:\S+)?$`, c)
}

deny contains msg if {
	some app in lib.ts_apps
	some field, want in {"type": "module", "private": true}
	object.get(package_json(app), field, null) != want
	msg := sprintf("apps/%s/package.json: set %q to %v", [app, field, want])
}

deny contains msg if {
	some app in lib.ts_apps
	object.get(package_json(app), ["engines", "node"], null) != root.engines.node
	msg := sprintf("apps/%s/package.json: engines.node should be %q, as in the root package.json", [app, root.engines.node])
}

# The anti-slop plugin pins @oxlint/plugins to oxlint's version (tools/oxlint/anti-slop/UPSTREAM.md).
deny contains msg if {
	some app in lib.ts_apps
	object.get(package_json(app), ["devDependencies", "oxlint"], null) != root.devDependencies.oxlint
	msg := sprintf("apps/%s/package.json: pin oxlint to %q, as the root does", [app, root.devDependencies.oxlint])
}

# Every step of CI's checks that a laptop can run is in a verify script: locallibrary-ci.yml's lint
# job, and repo-checks.yml.
checked_jobs contains [".github/workflows/locallibrary-ci.yml", "lint"]

checked_jobs contains [path, name] if {
	path := ".github/workflows/repo-checks.yml"
	some name, _ in lib.file(path).jobs
}

# The steps of the root's verify scripts (tools/verify/steps.json).
verify_commands contains c if {
	some commands in lib.file("tools/verify/steps.json")
	some c in commands
}

# Steps that set up the runner (the checkout, toolchains, caches, installs), which a laptop has done.
setup := {
	"actions/checkout",
	"pnpm/action-setup",
	"actions/setup-node",
	"actions/setup-go",
	"Swatinem/rust-cache",
	"taiki-e/install-action",
	"./.github/actions/apt-packages",
	"./.github/actions/install-dist",
	"rustup toolchain install",
	"pnpm install --frozen-lockfile",
	"pnpm install --frozen-lockfile --filter localnetwork-monorepo",
}

# The jobs and steps only CI runs, and why.
ci_only_steps := {
	"diagrams": "the drift check reads the pull request's body",
	"commits": "lefthook's commit-msg hook runs commitlint on each commit",
	"./.github/actions/patch-coverage": "it covers the lines a pull request changes, so it runs on pull requests only",
}

# How the verify scripts spell a step CI spells another way.
spelling := {
	"cargo doc --workspace --no-deps --locked": "cargo doc-strict",
	# Double quotes, which cmd.exe reads too.
	"cargo nextest run -p locallibrary --no-default-features --locked -E 'test(commands_come_with_the_features_built_in)'": "cargo nextest run -p locallibrary --no-default-features --locked -E \"test(commands_come_with_the_features_built_in)\"",
	"dist plan --output-format=json > \"$RUNNER_TEMP/plan-dist-manifest.json\"": "node tools/verify/release-cli.ts",
	"Check only locallibrary is released": "node tools/verify/release-cli.ts",
	"ShellCheck": "git ls-files -z \"*.sh\" | xargs -0 shellcheck",
	"crate-ci/typos": "typos",
	"lycheeverse/lychee-action": "git ls-files -z \"*.md\" | xargs -0 lychee --offline --include-fragments --no-progress",
}

deny contains msg if {
	some job_path in checked_jobs
	[path, name] := job_path
	not ci_only_steps[name]
	job := lib.file(path).jobs[name]
	some step in job.steps
	keys := step_keys(step)
	count(keys & (setup | object.keys(ci_only_steps))) == 0
	not verified(keys, object.get(step, "working-directory", object.get(job, ["defaults", "run", "working-directory"], "")))
	msg := sprintf("%s: add %v (job %s) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why", [path, sort(keys), name])
}

# A step's name, command and action, any of which can match.
step_keys(step) := {k |
	some k in [object.get(step, "name", ""), trim_space(object.get(step, "run", "")), split(object.get(step, "uses", ""), "@")[0]]
	k != ""
}

verified(keys, dir) if {
	some k in keys
	spelled(k, dir) in verify_commands
}

spelled(k, "") := object.get(spelling, k, k)

# A step's working directory, or its job's, as npm's --prefix.
spelled(k, dir) := sprintf("%s --prefix %s", [k, dir]) if dir != ""

# lefthook.yml formats and lints staged files and checks and tests before a push, per app and per
# package.
hooks := {
	"pre-commit": {"oxfmt", "oxlint"},
	"pre-push": {"pnpm check", "pnpm test"},
}

hooked := {sprintf("apps/%s/", [app]) | some app in lib.ts_apps} | {trim_suffix(p, "package.json") | some p in lib.paths("packages/*/package.json")}

# The hook jobs a folder goes without, and why.
hook_exceptions := {
	# Its disk-image tests failed pushes and hung them for hours; CI's atstore job runs them on
	# every pull request that touches it.
	"packages/atstore/": {"pre-push": {"pnpm test"}},
}

deny contains msg if {
	some dir in hooked
	some hook, tools in hooks
	some tool in tools
	not tool in object.get(hook_exceptions, [dir, hook], set())
	not hook_runs(hook, dir, tool)
	msg := sprintf("lefthook.yml: add a %s job for %s that runs %s", [hook, dir, tool])
}

# pre-commit's format and lint jobs run in its group, so the whitespace check after it sees what they
# fixed.
deny contains msg if {
	some job in lib.file("lefthook.yml")["pre-commit"].jobs
	some dir in hooked
	runs_in(job, dir)
	some tool in hooks["pre-commit"]
	contains(job.run, tool)
	msg := sprintf("lefthook.yml: move pre-commit's %s job for %s into its group, before the whitespace check", [tool, dir])
}

# Each pre-push job runs on its app's paths in ci.yml's changes job (the filter named after its
# folder), as lefthook matches them: its `*` crosses folders, so `tools/*.ts` takes in
# `tools/**/*.ts`.
deny contains msg if {
	some f in workflows.paths_filters
	f.path == ".github/workflows/ci.yml"
	some dir in hooked
	endswith(dir, sprintf("/%s/", [f.name]))
	some job in hook_jobs("pre-push")
	runs_in(job, dir)
	some path in f.globs
	not ci_only(path)
	not pushed(job, path)
	msg := sprintf("lefthook.yml: add %s to pre-push job %q's glob, as ci.yml's %s filter has it", [path, job.name, f.name])
}

# The paths only CI reads, and the ones a filter leaves out.
ci_only(path) if startswith(path, "!")

ci_only(path) if startswith(path, ".github/workflows/")

ci_only(path) if startswith(path, ".github/actions/")

ci_only("tools/coverage/patch.json")

# A job with no glob runs on every push.
pushed(job, _) if not job.glob

pushed(job, path) if {
	some pattern in lib.string_set(job.glob)
	glob.match(pattern, ["\u0000"], path)
}

# The hooks run the committing worktree's lefthook, not the path of the one that installed them last.
deny contains msg if {
	lib.file("lefthook.yml").lefthook != "pnpm exec lefthook"
	msg := "lefthook.yml: set `lefthook: pnpm exec lefthook`, so every worktree's hooks run its own lefthook"
}

deny contains msg if {
	not lib.file("lefthook.yml").lefthook
	msg := "lefthook.yml: set `lefthook: pnpm exec lefthook`, so every worktree's hooks run its own lefthook"
}

hook_runs(hook, dir, tool) if {
	some job in hook_jobs(hook)
	runs_in(job, dir)
	contains(job.run, tool)
}

# A hook's jobs, and the jobs of its groups (pre-commit runs its formatters as one, before the check
# over what they staged).
hook_jobs(hook) := {job | some job in lib.file("lefthook.yml")[hook].jobs} | {job |
	some group in lib.file("lefthook.yml")[hook].jobs
	some job in group.group.jobs
}

runs_in(job, dir) if job.root == dir

# The pre-push jobs cd into their folder: a `root` would hide the shared files outside it, which
# their globs list, from them.
runs_in(job, dir) if startswith(job.run, sprintf("cd %s && ", [dir]))
