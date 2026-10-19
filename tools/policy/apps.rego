# Each TypeScript app has what the root's scripts, CI and the git hooks expect of it
# (.github/NEW_PROJECT.md, "A new TypeScript app").
package apps

import data.lib

# The scripts the root's `pnpm -r` scripts and the CI workflows call.
scripts := {"dev", "build", "check", "lint", "format", "format:check", "knip", "test", "test:coverage", "verify"}

# The checks `verify` has to run, as CI's verify jobs do.
verify_steps := {"pnpm format:check", "pnpm lint", "pnpm check", "pnpm knip"}

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

# lefthook.yml formats and lints staged files and checks and tests before a push, per app.
hooks := {
	"pre-commit": {"oxfmt", "oxlint"},
	"pre-push": {"pnpm check", "pnpm test"},
}

deny contains msg if {
	some app in lib.ts_apps
	some hook, tools in hooks
	some tool in tools
	not hook_runs(hook, sprintf("apps/%s/", [app]), tool)
	msg := sprintf("lefthook.yml: add a %s job with root apps/%s/ that runs %s", [hook, app, tool])
}

hook_runs(hook, root_dir, tool) if {
	some job in lib.file("lefthook.yml")[hook].jobs
	job.root == root_dir
	contains(job.run, tool)
}
