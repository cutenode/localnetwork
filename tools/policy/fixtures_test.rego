# A small repository that passes every policy, as conftest would give it to them, for the tests
# to break one file at a time.
package fixtures

passed_if := "always() && (contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') || (needs.changes.outputs.run == 'true' && contains(needs.*.result, 'skipped')))"

passed_step := {"run": "exit 1"}

base_step := {"id": "base", "uses": "./.github/actions/last-passed"}

# The changes job's steps, filtering on `filter`.
changes_steps(filter) := [base_step, {"uses": "dorny/paths-filter@abc", "with": {"base": "${{ steps.base.outputs.sha }}", "filters": filter}}]

# A gated workflow whose changes job filters on `filter`.
workflow(on, name, filter) := {
	"true": on,
	"concurrency": {"group": name, "cancel-in-progress": true},
	"jobs": {
		"changes": {"steps": changes_steps(filter)},
		"passed": {"name": name, "if": passed_if, "needs": ["changes"], "steps": [passed_step]},
	},
}

package_fields := {
	"edition": {"workspace": true},
	"rust-version": {"workspace": true},
	"license": {"workspace": true},
	"repository": {"workspace": true},
}

repo := [
	{"path": "package.json", "contents": {"engines": {"node": ">=24.12"}, "devDependencies": {"oxlint": "1.86.0"}}},
	{"path": "apps/site/package.json", "contents": {
		"type": "module",
		"private": true,
		"engines": {"node": ">=24.12"},
		"devDependencies": {"oxlint": "1.86.0"},
		"scripts": {
			"dev": "vite dev", "build": "vite build", "check": "svelte-check", "lint": "oxlint", "format": "oxfmt",
			"format:check": "oxfmt --check", "knip": "knip", "test": "vitest run", "test:coverage": "vitest run --coverage",
			"verify": "pnpm format:check && pnpm lint && pnpm check && pnpm knip && pnpm test:coverage",
		},
	}},
	{"path": "apps/site/Dockerfile", "contents": []},
	{"path": "packages/kit/package.json", "contents": {"name": "kit"}},
	{"path": "/scratch/railway-project.json", "contents": {"resources": [{
		"type": "service", "name": "site",
		"build": {"dockerfilePath": "apps/site/Dockerfile"},
		"deploy": {"healthcheckPath": "/healthz", "multiRegionConfig": {"us-east4": {"numReplicas": 2}}},
	}]}},
	{"path": "apps/tool/Cargo.toml", "contents": {"package": object.union({"name": "tool"}, package_fields), "lints": {"workspace": true}}},
	{"path": "Cargo.toml", "contents": {
		"workspace": {"members": ["apps/tool", "crates/*", "plugins/*", "vendor/pdb"]},
		"profile": {"dev": {"package": {"sha2": {"opt-level": 3}}}},
	}},
	{"path": "/scratch/Cargo.lock.toml", "contents": {"package": [{"name": "sha2"}, {"name": "tool"}]}},
	{"path": "crates/loclib-x/Cargo.toml", "contents": {"package": object.union({"name": "loclib-x", "readme": "README.md"}, package_fields), "lints": {"workspace": true}}},
	{"path": "plugins/p/Cargo.toml", "contents": {"package": object.union({"name": "locallibrary-plugin-p", "publish": false}, package_fields), "lints": {"workspace": true}}},
	{"path": "vendor/pdb/Cargo.toml", "contents": {"package": {"name": "pdb", "edition": "2021", "rust-version": {"workspace": true}}, "lints": {"workspace": true}}},
	{"path": "lefthook.yml", "contents": {
		"pre-commit": {"jobs": [{"root": "apps/site/", "run": "pnpm exec oxfmt {staged_files}"}, {"root": "apps/site/", "run": "pnpm exec oxlint {staged_files}"}]},
		"pre-push": {"jobs": [{"root": "apps/site/", "run": "pnpm check"}, {"root": "apps/site/", "run": "pnpm test"}]},
	}},
	{"path": ".github/workflows/site-ci.yml", "contents": {
		"true": {"pull_request": null, "push": {"branches": ["main"]}},
		"concurrency": {"group": "ci", "cancel-in-progress": true},
		"jobs": {
			"changes": {"steps": changes_steps("run:\n  - 'apps/**'\n  - 'packages/**'\n  - '!**/*.md'\n")},
			"verify": {"needs": "changes"},
			"e2e": {"needs": "changes", "uses": "./.github/workflows/site-e2e.yml", "permissions": {"contents": "read", "pull-requests": "write"}},
			"passed": {"name": "Site CI passed", "if": passed_if, "needs": ["changes", "verify", "e2e"], "steps": [passed_step]},
		},
	}},
	{"path": ".github/workflows/site-e2e.yml", "contents": {
		"true": {"workflow_call": null, "schedule": [{"cron": "0 5 * * 1"}]},
		"jobs": {
			"test": {"permissions": {"contents": "read"}},
			"report": {"needs": "test", "continue-on-error": true, "permissions": {"pull-requests": "write"}},
		},
	}},
	{"path": ".github/workflows/rust.yml", "contents": workflow("pull_request", "Rust passed", "locallibrary:\n  - 'crates/**'\n  - 'plugins/**'\n  - 'lexicons/computer/localnetwork/dj/**'\n  - '!Cargo.lock'\n  - '!**/*.md'\n")},
	{"path": ".github/workflows/deps.yml", "contents": workflow({"pull_request": {"branches": ["main"]}}, "Deps passed", "run:\n  - 'vendor/**'\n")},
	{"path": ".github/workflows/lint.yml", "contents": workflow(["push", "pull_request"], "Lint passed", "run: '.github/**'\n")},
	{"path": ".github/workflows/labels.yml", "contents": {"true": ["push"], "jobs": {"sync": {}}}},
	{"path": ".github/labels.yml", "contents": {"name": "site"}},
	{"path": ".github/labels.yml", "contents": {"name": "tool"}},
	{"path": ".github/labels.yml", "contents": {"name": "docs"}},
	{"path": ".github/labeler.yml", "contents": {
		"site": [{"changed-files": [{"any-glob-to-any-file": ["apps/site/**"]}]}],
		"tool": [{"changed-files": [{"any-glob-to-any-file": "apps/tool/**"}]}],
		"docs": [{"changed-files": [{"all-globs-to-any-file": ["**/*.md", "!apps/**"]}, {"any-glob-to-any-file": "docs/**/*.txt"}]}],
	}},
	{"path": ".github/ISSUE_TEMPLATE/site-bug.yml", "contents": {"labels": ["bug", "site"]}},
	{"path": ".github/ISSUE_TEMPLATE/tool-bug.yml", "contents": {"labels": ["bug", "tool"]}},
	{"path": ".github/ISSUE_TEMPLATE/feature_request.yml", "contents": {"body": [{"id": "tool", "attributes": {"options": ["site", "tool", "Something new"]}}]}},
	{"path": ".github/rulesets/protect-main.json", "contents": {"rules": [
		{"type": "deletion"},
		{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "Site CI passed"}, {"context": "Rust passed"}, {"context": "Deps passed"}, {"context": "Lint passed"}]}},
	]}},
	{"path": "lexicons/computer/localnetwork/tool/record.json", "contents": {"id": "computer.localnetwork.tool.record"}},
	{"path": "lexicons/computer/localnetwork/toolPublishing.json", "contents": {"id": "computer.localnetwork.toolPublishing"}},
	{"path": "lexicons/computer/localnetwork/dj/set.json", "contents": {"id": "computer.localnetwork.dj.set"}},
	{"path": "lexicons/community/lexicon/location/address.json", "contents": {"id": "community.lexicon.location.address"}},
	{"path": "/scratch/embedded-files.json", "contents": {"files": ["lexicons/computer/localnetwork/dj/set.json", "plugins/p/locallibrary-plugin.toml"]}},
	{"path": "/scratch/tracked-files.json", "contents": {"files": ["apps/site/src/app.ts", "README.md", "crates/x/README.md", "docs/b.txt", "apps/tool/x.rs"]}},
]

# The repository with the file at `path` (or every item of a list file) replaced by `contents`.
with_file(path, contents) := array.concat([f | some f in repo; f.path != path], [{"path": path, "contents": contents}])

without_file(path) := [f | some f in repo; f.path != path]

file(path) := f.contents if {
	some f in repo
	f.path == path
}
