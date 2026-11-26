# pnpm's supply-chain exceptions in pnpm-workspace.yaml each name one version: an install script
# allowed to run, or a package let past the trust policy or the release age gate. A bare name would
# let every later release through unreviewed. The settings themselves stay at harden's strict level.
package pnpm

import data.lib

exact := `^(@[^/@]+/)?[^/@]+@[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$`

exceptions contains ["allowBuilds", name] if {
	some name, allowed in lib.file("pnpm-workspace.yaml").allowBuilds
	allowed == true
}

exceptions contains [key, name] if {
	some key in ["trustPolicyExclude", "minimumReleaseAgeExclude"]
	some name in lib.file("pnpm-workspace.yaml")[key]
}

deny contains msg if {
	some [key, name] in exceptions
	not regex.match(exact, name)
	msg := sprintf("pnpm-workspace.yaml: %s's %s has to name one version (name@x.y.z), or every later release gets through unreviewed", [key, name])
}

# The package managers' supply-chain settings stay at @lavamoat/harden's strict level: pnpm's for
# the workspace, npm's for Bombadil's own project. All but the script runners (p_runner, n_runner),
# whose script shell doesn't run under Windows' cmd.exe, which the Windows CI jobs use. `harden
# check` only takes a whole level, so this reads its report.
deny contains msg if {
	some project in lib.harden
	unmet := [check.id | some check in project.report.checks; check.satisfied != 1; not regex.match(`^(p|n)_runner$`, check.id)]
	count(unmet) > 0
	msg := sprintf("%s: weaker than `harden defaults -p %s -l strict` sets it (all but the runner): %s", [project.folder, project.manager, concat(" ", unmet)])
}

deny contains msg if {
	some project in lib.harden
	count(object.get(project.report, "checks", [])) == 0
	msg := sprintf("%s: `harden check -p %s` reported no checks", [project.folder, project.manager])
}
