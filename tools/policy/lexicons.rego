# Lexicons are filed by NSID, and a new group under computer.localnetwork is a tool's own
# (.github/NEW_PROJECT.md, "Lexicons, tokens and the website's tool pages").
package lexicons

import data.lib

documents := lib.paths("lexicons/**/*.json")

# The website's records, which predate tool groups, and the groups several tools share. A new
# shared group goes here on purpose, named for what its records are.
shared := {"audit", "booking", "defs", "directory", "dj", "epk", "follow", "listing", "moderation", "people", "permissions", "planner", "project", "show", "space", "staff"}

deny contains msg if {
	some path in documents
	want := replace(trim_suffix(trim_prefix(path, "lexicons/"), ".json"), "/", ".")
	object.get(lib.file(path), "id", null) != want
	msg := sprintf("%s: its id should be %s, the NSID its path spells", [path, want])
}

deny contains msg if {
	some path in documents
	startswith(path, "lexicons/computer/localnetwork/")
	group := trim_suffix(split(trim_prefix(path, "lexicons/computer/localnetwork/"), "/")[0], ".json")
	not allowed(group)
	msg := sprintf("%s: put a tool's records under computer.localnetwork.<tool>, or add %q to the shared groups in tools/policy/lexicons.rego", [path, group])
}

allowed(group) if group in shared

allowed(group) if group in lib.apps

# A tool's OAuth permission set sits beside the groups, as locallibraryPublishing does.
allowed(group) if {
	some app in lib.apps
	group == sprintf("%sPublishing", [app])
}
