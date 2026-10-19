# An app that runs as a service has a service in .railway/railway.ts that builds its Dockerfile
# (.github/NEW_PROJECT.md, "A new TypeScript app"). tools/policy.sh evaluates that file into
# railway-project.json, the project as Railway's SDK describes it.
package railway

import data.lib

project := f.contents if {
	some f in input
	regex.match(`(^|/)railway-project\.json$`, f.path)
}

services := [r | some r in project.resources; r.type == "service"]

deny contains msg if {
	some dockerfile in lib.paths("apps/*/Dockerfile")
	not dockerfile in {s.build.dockerfilePath | some s in services}
	msg := sprintf("%s: add a service to .railway/railway.ts that builds it", [dockerfile])
}

deny contains msg if {
	some s in services
	not object.get(s, ["build", "dockerfilePath"], "") in lib.paths("apps/*/Dockerfile")
	msg := sprintf(".railway/railway.ts: service %q should build an app's Dockerfile, named from the repository root", [s.name])
}

deny contains msg if {
	some s in services
	object.get(s, ["deploy", "healthcheckPath"], "") == ""
	msg := sprintf(".railway/railway.ts: service %q needs a healthcheck", [s.name])
}

# The website keeps SQLite and its background jobs in one process (apps/web/README.md, Deploying).
deny contains msg if {
	some s in services
	s.build.dockerfilePath == "apps/web/Dockerfile"
	replicas := sum([r.numReplicas | some r in object.get(s, ["deploy", "multiRegionConfig"], {})])
	replicas != 1
	msg := sprintf(".railway/railway.ts: service %q is the website, which runs as exactly one instance", [s.name])
}
