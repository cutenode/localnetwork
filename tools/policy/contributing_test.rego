package contributing_test

import data.contributing

# A pull request workflow with a step of each kind CONTRIBUTING covers, and one it calls.
ci := {"path": ".github/workflows/ci.yml", "contents": {
	"true": {"pull_request": null},
	"jobs": {
		"checks": {"steps": [
			# Setup.
			{"uses": "actions/checkout@abc"},
			# A step of `pnpm verify`.
			{"run": "pnpm knip"},
			# A step of `pnpm verify:repo`, spelled another way.
			{"uses": "crate-ci/typos@abc"},
			# Named in CONTRIBUTING, spelled another way.
			{"run": "\"$(go env GOPATH)/bin/actionlint\""},
			# Named in CONTRIBUTING as it is.
			{"run": "pnpm audit --prod"},
			# Unlisted by its name.
			{"name": "Pull the images", "run": "docker pull postgres"},
			# The budgets.
			{"run": "node tools/budgets/cli.ts check web"},
		]},
		# A step of `pnpm verify:repo`, with npm's --prefix for its job's folder.
		"bombadil": {"defaults": {"run": {"working-directory": "apps/b"}}, "steps": [{"run": "npm ci"}]},
		# Unlisted as a whole job.
		"changes": {"steps": [{"run": "gh api repos"}]},
		"site": {"uses": "./.github/workflows/site-ci.yml"},
	},
}}

site := {"path": ".github/workflows/site-ci.yml", "contents": {
	"true": {"workflow_call": null},
	"defaults": {"run": {"working-directory": "apps/site"}},
	"jobs": {
		"verify": {"steps": [
			# A step of the app's own `verify`, which `pnpm verify` runs.
			{"run": "pnpm lint"},
			# Named in CONTRIBUTING with pnpm's -C.
			{"run": "pnpm test:e2e"},
			# Named in CONTRIBUTING in a subshell.
			{"run": "selene .", "working-directory": "apps/site/lua"},
			# At the root again.
			{"run": "pnpm knip", "working-directory": "."},
		]},
		# A report, which can't fail.
		"report": {"steps": [{"run": "post a comment"}]},
	},
}}

repo := [
	ci,
	site,
	{"path": "/scratch/contributing.json", "contents": {"commands": [
		"pnpm verify          # the apps",
		"pnpm verify:repo",
		"actionlint",
		"pnpm audit --prod",
		"pnpm -C apps/site test:e2e --project chromium",
		"(cd apps/site/lua && selene .)",
	]}},
	{"path": "tools/verify/steps.json", "contents": {
		"verify": ["pnpm knip", "pnpm -r --no-bail verify"],
		"verify:repo": ["typos", "npm ci --prefix apps/b"],
		"verify:rust": ["cargo deny"],
	}},
	{"path": "apps/site/package.json", "contents": {"scripts": {"verify": "pnpm lint && pnpm test"}}},
	{"path": "packages/kit/package.json", "contents": {"name": "kit"}},
]

unlisted := {".github/workflows/ci.yml#changes": "it picks the jobs", "Pull the images": "the tests pull them"}

with_file(path, contents) := array.concat([f | some f in repo; f.path != path], [{"path": path, "contents": contents}])

msg(step) := sprintf(".github/workflows/ci.yml: give %v (job checks) in .github/CONTRIBUTING.md, or add it to unlisted in tools/policy/contributing.rego with why", [step])

test_every_step_covered if {
	count(contributing.deny) == 0 with input as repo with contributing.unlisted as unlisted
}

test_step_contributing_misses if {
	broken := json.patch(ci.contents, [{"op": "add", "path": "/jobs/checks/steps/-", "value": {"run": "cargo deny"}}])
	denied := contributing.deny with input as with_file(ci.path, broken) with contributing.unlisted as unlisted
	denied == {msg(["cargo deny"])}
}

# A verify script's steps count only when CONTRIBUTING names the script.
test_verify_script_not_named if {
	commands := {"commands": ["pnpm verify", "actionlint", "pnpm audit --prod", "pnpm -C apps/site test:e2e", "(cd apps/site/lua && selene .)"]}
	denied := contributing.deny with input as with_file("/scratch/contributing.json", commands) with contributing.unlisted as unlisted
	denied == {
		msg(["crate-ci/typos"]),
		".github/workflows/ci.yml: give [\"npm ci\"] (job bombadil) in .github/CONTRIBUTING.md, or add it to unlisted in tools/policy/contributing.rego with why",
	}
}

test_unlisted_entry_that_matches_nothing if {
	stale := object.union(unlisted, {"Gone": "it was removed"})
	denied := contributing.deny with input as repo with contributing.unlisted as stale
	denied == {"tools/policy/contributing.rego: unlisted[\"Gone\"] matches no job or step of a pull request's workflows; remove it"}
}

test_each_unlisted_says_why if {
	every why in contributing.unlisted {
		count(why) > 0
	}
}
