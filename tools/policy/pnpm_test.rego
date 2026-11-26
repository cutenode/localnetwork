package pnpm_test

import data.fixtures
import data.pnpm

workspace(contents) := fixtures.with_file("pnpm-workspace.yaml", contents)

test_fixture_passes if {
	count(pnpm.deny) == 0 with input as fixtures.repo
}

test_exact_versions_pass if {
	count(pnpm.deny) == 0 with input as workspace({
		"allowBuilds": {"lefthook@2.2.2": true, "@scope/pkg@1.0.0-rc.1": true, "esbuild": false},
		"trustPolicyExclude": ["@yarnpkg/libzip@3.2.2"],
		"minimumReleaseAgeExclude": ["chromium-bidi@157.0.8090-0"],
	})
}

test_bare_build_approval if {
	denied := pnpm.deny with input as workspace({"allowBuilds": {"lefthook": true}})
	denied == {"pnpm-workspace.yaml: allowBuilds's lefthook has to name one version (name@x.y.z), or every later release gets through unreviewed"}
}

test_range_build_approval if {
	denied := pnpm.deny with input as workspace({"allowBuilds": {"@scope/pkg@^1.0.0": true}})
	denied == {"pnpm-workspace.yaml: allowBuilds's @scope/pkg@^1.0.0 has to name one version (name@x.y.z), or every later release gets through unreviewed"}
}

test_bare_exclusions if {
	denied := pnpm.deny with input as workspace({"trustPolicyExclude": ["chokidar"], "minimumReleaseAgeExclude": ["@atproto/*"]})
	denied == {
		"pnpm-workspace.yaml: trustPolicyExclude's chokidar has to name one version (name@x.y.z), or every later release gets through unreviewed",
		"pnpm-workspace.yaml: minimumReleaseAgeExclude's @atproto/* has to name one version (name@x.y.z), or every later release gets through unreviewed",
	}
}

harden(checks) := fixtures.with_file("/scratch/harden.json", {"harden": [{"manager": "pnpm", "folder": ".", "report": {"checks": checks}}]})

test_harden_runner_alone_unmet_passes if {
	count(pnpm.deny) == 0 with input as harden([{"id": "p_age", "satisfied": 1}, {"id": "p_runner", "satisfied": 0}])
}

test_harden_unmet_check if {
	denied := pnpm.deny with input as harden([{"id": "p_age", "satisfied": 0}, {"id": "p_scripts", "satisfied": 0}, {"id": "p_runner", "satisfied": 0}])
	denied == {".: weaker than `harden defaults -p pnpm -l strict` sets it (all but the runner): p_age p_scripts"}
}

test_harden_empty_report if {
	denied := pnpm.deny with input as fixtures.with_file("/scratch/harden.json", {"harden": [{"manager": "npm", "folder": "apps/b", "report": {}}]})
	denied == {"apps/b: `harden check -p npm` reported no checks"}
}
