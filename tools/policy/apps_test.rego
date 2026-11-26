package apps_test

import data.apps
import data.fixtures

site := fixtures.file("apps/site/package.json")

test_fixture_passes if {
	count(apps.deny) == 0 with input as fixtures.repo
}

test_missing_script if {
	broken := json.remove(site, ["scripts/lint"])
	"apps/site/package.json: add a \"lint\" script" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_verify_skips_a_check if {
	broken := json.patch(site, [{"op": "replace", "path": "/scripts/verify", "value": "pnpm lint && pnpm check && pnpm test"}])
	"apps/site/package.json: `verify` should run `pnpm format:check`" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_verify_skips_tests if {
	broken := json.patch(site, [{"op": "replace", "path": "/scripts/verify", "value": "pnpm format:check && pnpm lint && pnpm check"}])
	"apps/site/package.json: `verify` should run its tests" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_not_a_module if {
	broken := object.union(site, {"type": "commonjs"})
	"apps/site/package.json: set \"type\" to module" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_missing_fields if {
	broken := json.remove(site, ["type", "private", "engines", "devDependencies"])
	denied := apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
	"apps/site/package.json: set \"type\" to module" in denied
	"apps/site/package.json: set \"private\" to true" in denied
	"apps/site/package.json: engines.node should be \">=24.12\", as in the root package.json" in denied
	"apps/site/package.json: pin oxlint to \"1.86.0\", as the root does" in denied
}

test_other_node if {
	broken := object.union(site, {"engines": {"node": ">=22"}})
	"apps/site/package.json: engines.node should be \">=24.12\", as in the root package.json" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_oxlint_range if {
	broken := object.union(site, {"devDependencies": {"oxlint": "^1.86.0"}})
	"apps/site/package.json: pin oxlint to \"1.86.0\", as the root does" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_lefthook_unset if {
	broken := json.remove(fixtures.file("lefthook.yml"), ["lefthook"])
	"lefthook.yml: set `lefthook: pnpm exec lefthook`, so every worktree's hooks run its own lefthook" in apps.deny with input as fixtures.with_file("lefthook.yml", broken)
}

test_lefthook_other_command if {
	broken := object.union(fixtures.file("lefthook.yml"), {"lefthook": "/usr/local/bin/lefthook"})
	"lefthook.yml: set `lefthook: pnpm exec lefthook`, so every worktree's hooks run its own lefthook" in apps.deny with input as fixtures.with_file("lefthook.yml", broken)
}

test_missing_hook if {
	hooks := fixtures.file("lefthook.yml")
	broken := object.union(hooks, {"pre-push": {"jobs": [{"root": "apps/site/", "run": "pnpm check"}]}})
	"lefthook.yml: add a pre-push job for apps/site/ that runs pnpm test" in apps.deny with input as fixtures.with_file("lefthook.yml", broken)
}

test_pre_commit_job_outside_its_group if {
	hooks := fixtures.file("lefthook.yml")
	outside := object.union(hooks, {"pre-commit": {"jobs": array.concat(hooks["pre-commit"].jobs, [{"root": "apps/site/", "run": "pnpm exec oxlint {staged_files}"}])}})
	denied := apps.deny with input as fixtures.with_file("lefthook.yml", outside)
	denied == {"lefthook.yml: move pre-commit's oxlint job for apps/site/ into its group, before the whitespace check"}
}

test_package_without_hooks if {
	repo := fixtures.with_file("packages/other/package.json", {"name": "other"})
	"lefthook.yml: add a pre-commit job for packages/other/ that runs oxfmt" in apps.deny with input as repo
}

test_pre_push_job_in_its_folder if {
	hooks := fixtures.file("lefthook.yml")
	cd := object.union(hooks, {"pre-push": {"jobs": [
		{"run": "cd apps/site/ && pnpm check && pnpm test"},
		{"run": "cd packages/kit/ && pnpm check && pnpm test"},
	]}})
	count(apps.deny) == 0 with input as fixtures.with_file("lefthook.yml", cd)
}

test_atstore_tests_left_to_ci if {
	hooks := fixtures.file("lefthook.yml")
	pre_commit := [{"group": {"jobs": array.concat(hooks["pre-commit"].jobs[0].group.jobs, [
		{"root": "packages/atstore/", "run": "pnpm exec oxfmt {staged_files}"},
		{"root": "packages/atstore/", "run": "pnpm exec oxlint {staged_files}"},
	])}}]
	repo := fixtures.with_file("packages/atstore/package.json", {"name": "atstore"})
	without_tests := object.union(hooks, {
		"pre-commit": {"jobs": pre_commit},
		"pre-push": {"jobs": array.concat(hooks["pre-push"].jobs, [{"run": "cd packages/atstore/ && pnpm check"}])},
	})
	count(apps.deny) == 0 with input as with_hooks(repo, without_tests)

	without_check := object.union(hooks, {"pre-commit": {"jobs": pre_commit}})
	denied := apps.deny with input as with_hooks(repo, without_check)
	denied == {"lefthook.yml: add a pre-push job for packages/atstore/ that runs pnpm check"}
}

# ci.yml with a filter for apps/site/, and its pre-push job.
site_filter := "site:\n  - 'apps/site/**'\n  - 'tools/**/*.ts'\n  - '.github/workflows/site-ci.yml'\n  - '.github/actions/size/**'\n  - 'tools/coverage/patch.json'\n  - '!**/*.md'\n"

site_push := object.union(fixtures.file("lefthook.yml"), {"pre-push": {"jobs": [
	{"name": "check site", "glob": ["apps/site/**", "tools/*.ts"], "run": "cd apps/site/ && pnpm check && pnpm test"},
	{"root": "packages/kit/", "run": "pnpm check"}, {"root": "packages/kit/", "run": "pnpm test"},
]}})

test_pre_push_glob_has_the_filter if {
	repo := fixtures.with_file(".github/workflows/ci.yml", fixtures.workflow("pull_request", "CI passed", site_filter))
	count(apps.deny) == 0 with input as with_hooks(repo, site_push)
}

test_pre_push_glob_misses_a_path if {
	filter := concat("", [site_filter, "  - 'docs/new.md'\n"])
	repo := fixtures.with_file(".github/workflows/ci.yml", fixtures.workflow("pull_request", "CI passed", filter))
	denied := apps.deny with input as with_hooks(repo, site_push)
	denied == {"lefthook.yml: add docs/new.md to pre-push job \"check site\"'s glob, as ci.yml's site filter has it"}
}

test_pre_push_job_without_a_glob if {
	everything := object.union(fixtures.file("lefthook.yml"), {"pre-push": {"jobs": [
		{"name": "check site", "run": "cd apps/site/ && pnpm check && pnpm test"},
		{"root": "packages/kit/", "run": "pnpm check"}, {"root": "packages/kit/", "run": "pnpm test"},
	]}})
	filter := concat("", [site_filter, "  - 'docs/new.md'\n"])
	repo := fixtures.with_file(".github/workflows/ci.yml", fixtures.workflow("pull_request", "CI passed", filter))
	count(apps.deny) == 0 with input as with_hooks(repo, everything)
}

# CI's checks with a step of each kind the verify scripts are held to, and the scripts that run them.
ci_checks := array.concat(
	fixtures.repo,
	[
		{"path": "tools/verify/steps.json", "contents": {
			"verify": ["pnpm knip"],
			"verify:rust": ["cargo fmt --all --check", "cargo doc-strict"],
			"verify:repo": ["pnpm exec ast-grep scan", "git ls-files -z \"*.sh\" | xargs -0 shellcheck", "npm ci --prefix apps/b", "npm run check --prefix apps/c"],
		}},
		{"path": ".github/workflows/locallibrary-ci.yml", "contents": {"jobs": {
			"lint": {"steps": [
				{"uses": "actions/checkout@abc"},
				{"run": "cargo fmt --all --check"},
				{"run": "cargo doc --workspace --no-deps --locked", "env": {"RUSTDOCFLAGS": "-D warnings"}},
			]},
			"test": {"steps": [{"run": "cargo coverage"}]},
		}}},
		{"path": ".github/workflows/repo-checks.yml", "contents": {"jobs": {
			"policy": {"steps": [
				{"run": "pnpm exec ast-grep scan\n"},
				{"uses": "./.github/actions/patch-coverage"},
				{"name": "ShellCheck", "run": "docker run shellcheck"},
			]},
			"bombadil": {"defaults": {"run": {"working-directory": "apps/b"}}, "steps": [{"run": "npm ci"}, {"run": "npm run check", "working-directory": "apps/c"}]},
			"knip": {"steps": [{"run": "pnpm knip"}]},
			"diagrams": {"steps": [{"run": "node tools/readmes/cli.ts drift"}]},
		}}},
	],
)

test_ci_steps_in_verify if {
	count(apps.deny) == 0 with input as ci_checks
}

test_ci_step_missing_from_verify if {
	steps := {"verify": ["pnpm knip"], "verify:rust": ["cargo fmt --all --check"], "verify:repo": ["pnpm exec ast-grep scan"]}
	repo := array.concat([f | some f in ci_checks; f.path != "tools/verify/steps.json"], [{"path": "tools/verify/steps.json", "contents": steps}])
	denied := apps.deny with input as repo
	denied == {
		".github/workflows/locallibrary-ci.yml: add [\"cargo doc --workspace --no-deps --locked\"] (job lint) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why",
		".github/workflows/repo-checks.yml: add [\"ShellCheck\", \"docker run shellcheck\"] (job policy) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why",
		".github/workflows/repo-checks.yml: add [\"npm ci\"] (job bombadil) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why",
		".github/workflows/repo-checks.yml: add [\"npm run check\"] (job bombadil) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why",
	}
}

test_narrower_verify_step if {
	steps := object.union(file_in(ci_checks, "tools/verify/steps.json"), {"verify:rust": ["cargo fmt --all --check -- src/lib.rs", "cargo doc-strict"]})
	repo := array.concat([f | some f in ci_checks; f.path != "tools/verify/steps.json"], [{"path": "tools/verify/steps.json", "contents": steps}])
	denied := apps.deny with input as repo
	denied == {".github/workflows/locallibrary-ci.yml: add [\"cargo fmt --all --check\"] (job lint) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why"}
}

test_ci_only_steps if {
	denied := apps.deny with input as ci_checks with apps.ci_only_steps as {}
	denied == {
		".github/workflows/repo-checks.yml: add [\"./.github/actions/patch-coverage\"] (job policy) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why",
		".github/workflows/repo-checks.yml: add [\"node tools/readmes/cli.ts drift\"] (job diagrams) to tools/verify/steps.json, or to ci_only_steps in tools/policy/apps.rego with why",
	}
}

file_in(repo, path) := f.contents if {
	some f in repo
	f.path == path
}

with_hooks(repo, hooks) := array.concat([f | some f in repo; f.path != "lefthook.yml"], [{"path": "lefthook.yml", "contents": hooks}])
