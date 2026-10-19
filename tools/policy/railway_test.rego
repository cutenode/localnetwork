package railway_test

import data.fixtures
import data.railway

project := fixtures.file("/scratch/railway-project.json")

site := project.resources[0]

with_site(s) := fixtures.with_file("/scratch/railway-project.json", json.patch(project, [{"op": "replace", "path": "/resources/0", "value": s}]))

test_fixture_passes if {
	count(railway.deny) == 0 with input as fixtures.repo
}

test_dockerfile_without_service if {
	empty := json.patch(project, [{"op": "replace", "path": "/resources", "value": []}])
	"apps/site/Dockerfile: add a service to .railway/railway.ts that builds it" in railway.deny with input as fixtures.with_file("/scratch/railway-project.json", empty)
}

test_service_names_no_dockerfile if {
	broken := json.patch(site, [{"op": "replace", "path": "/build/dockerfilePath", "value": "Dockerfile"}])
	".railway/railway.ts: service \"site\" should build an app's Dockerfile, named from the repository root" in railway.deny with input as with_site(broken)
}

test_service_without_health_check if {
	broken := json.remove(site, ["deploy/healthcheckPath"])
	".railway/railway.ts: service \"site\" needs a healthcheck" in railway.deny with input as with_site(broken)
}

test_website_runs_once if {
	web := json.patch(site, [{"op": "replace", "path": "/build/dockerfilePath", "value": "apps/web/Dockerfile"}])
	repo := array.concat(with_site(web), [{"path": "apps/web/Dockerfile", "contents": []}])
	".railway/railway.ts: service \"site\" is the website, which runs as exactly one instance" in railway.deny with input as repo
}
