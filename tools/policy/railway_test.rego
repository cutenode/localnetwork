package railway_test

import data.fixtures
import data.railway

config := fixtures.file("apps/site/railway.json")

test_fixture_passes if {
	count(railway.deny) == 0 with input as fixtures.repo
}

test_dockerfile_without_config if {
	"apps/site: add a railway.json beside its Dockerfile" in railway.deny with input as fixtures.without_file("apps/site/railway.json")
}

test_config_names_no_dockerfile if {
	broken := json.patch(config, [{"op": "replace", "path": "/build/dockerfilePath", "value": "Dockerfile"}])
	"apps/site/railway.json: build.dockerfilePath should name the app's Dockerfile from the repository root" in railway.deny with input as fixtures.with_file("apps/site/railway.json", broken)
}

test_config_without_health_check if {
	broken := json.remove(config, ["deploy/healthcheckPath"])
	"apps/site/railway.json: set deploy.healthcheckPath" in railway.deny with input as fixtures.with_file("apps/site/railway.json", broken)
}

test_website_runs_once if {
	repo := array.concat(fixtures.repo, [{"path": "apps/web/railway.json", "contents": config}])
	"apps/web/railway.json: the website runs as exactly one instance" in railway.deny with input as repo
}
