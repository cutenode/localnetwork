# pnpm's supply-chain exceptions in pnpm-workspace.yaml each name one version: an install script
# allowed to run, or a package let past the trust policy or the release age gate. A bare name would
# let every later release through unreviewed. (tools/policy.sh runs `harden check` for the rest.)
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
