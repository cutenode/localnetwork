# Dependabot updates every lockfile: the folder of each one the repository tracks is in an entry
# for its ecosystem in .github/dependabot.yml.
package dependabot

import data.lib

ecosystems := {"Cargo.lock": "cargo", "package-lock.json": "npm", "pnpm-lock.yaml": "npm"}

# The folder a lockfile is in, as dependabot.yml names it: "/" or "/fuzz".
folder(path, name) := "/" if path == name

folder(path, name) := trim_suffix(sprintf("/%s", [path]), sprintf("/%s", [name])) if path != name

deny contains msg if {
	some path in lib.tracked_files
	parts := split(path, "/")
	name := parts[count(parts) - 1]
	ecosystem := ecosystems[name]
	dir := folder(path, name)
	not updated(ecosystem, dir)
	msg := sprintf(".github/dependabot.yml: add %s to the %s entry's directories, so Dependabot updates %s", [dir, ecosystem, path])
}

updated(ecosystem, dir) if {
	some update in lib.file(".github/dependabot.yml").updates
	update["package-ecosystem"] == ecosystem
	some pattern in (lib.strings(object.get(update, "directory", [])) | lib.strings(object.get(update, "directories", [])))
	lib.matches(pattern, dir)
}
