package apps_test

import data.apps
import data.fixtures

site := fixtures.file("apps/site/package.json")

test_fixture_passes if {
	count(apps.deny) == 0 with input as fixtures.repo
}

test_missing_script if {
	broken := json.remove(site, ["scripts/knip"])
	"apps/site/package.json: add a \"knip\" script" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_verify_skips_a_check if {
	broken := json.patch(site, [{"op": "replace", "path": "/scripts/verify", "value": "pnpm lint && pnpm check && pnpm knip && pnpm test"}])
	"apps/site/package.json: `verify` should run `pnpm format:check`" in apps.deny with input as fixtures.with_file("apps/site/package.json", broken)
}

test_verify_skips_tests if {
	broken := json.patch(site, [{"op": "replace", "path": "/scripts/verify", "value": "pnpm format:check && pnpm lint && pnpm check && pnpm knip"}])
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

test_missing_hook if {
	hooks := fixtures.file("lefthook.yml")
	broken := object.union(hooks, {"pre-push": {"jobs": [{"root": "apps/site/", "run": "pnpm check"}]}})
	"lefthook.yml: add a pre-push job with root apps/site/ that runs pnpm test" in apps.deny with input as fixtures.with_file("lefthook.yml", broken)
}
