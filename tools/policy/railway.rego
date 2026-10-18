# An app that runs as a service has a Railway config next to its Dockerfile
# (.github/NEW_PROJECT.md, "A new TypeScript app").
package railway

import data.lib

deny contains msg if {
	some dockerfile in lib.paths("apps/*/Dockerfile")
	app := split(dockerfile, "/")[1]
	not lib.file(sprintf("apps/%s/railway.json", [app]))
	msg := sprintf("apps/%s: add a railway.json beside its Dockerfile", [app])
}

deny contains msg if {
	some path in lib.paths("apps/*/railway.json")
	dockerfile := object.get(lib.file(path), ["build", "dockerfilePath"], "")
	not dockerfile in lib.paths("apps/*/Dockerfile")
	msg := sprintf("%s: build.dockerfilePath should name the app's Dockerfile from the repository root", [path])
}

deny contains msg if {
	some path in lib.paths("apps/*/railway.json")
	some field in ["healthcheckPath", "numReplicas"]
	not lib.file(path).deploy[field]
	msg := sprintf("%s: set deploy.%s", [path, field])
}

# The website keeps SQLite and its background jobs in one process (apps/web/README.md, Deploying).
deny contains msg if {
	lib.file("apps/web/railway.json").deploy.numReplicas != 1
	msg := "apps/web/railway.json: the website runs as exactly one instance"
}
