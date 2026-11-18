# Lexicons are filed by NSID, and a new group under computer.localnetwork is a tool's own
# (.github/NEW_PROJECT.md, "Lexicons, tokens and the website's tool pages").
package lexicons

import data.lib

# The lexicon documents: every JSON file under lexicons/ except the manifest `lex install` pins the network ones in.
documents := lib.paths("lexicons/**/*.json") - {"lexicons/network.json"}

# The website's records, which predate tool groups, and the groups several tools share. A new
# shared group goes here on purpose, named for what its records are.
shared := {"audit", "booking", "defs", "directory", "dj", "epk", "flags", "follow", "listing", "moderation", "party", "people", "performer", "permissions", "planner", "profile", "project", "section", "show", "space", "staff", "venue"}

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

# Other authorities' lexicons come from the network, pinned in the manifest `lex install` reads, never vendored. The
# three below are compiled into locallibrary until it resolves them at runtime (LOC-804).
compiled_into_locallibrary := {
	"lexicons/app/userinput/discussion.json",
	"lexicons/com/atproto/repo/strongRef.json",
	"lexicons/community/lexicon/location/address.json",
}

deny contains msg if {
	some path in documents
	not startswith(path, "lexicons/computer/")
	not path in compiled_into_locallibrary
	msg := sprintf("%s: other authorities' lexicons come from the network, not the repo: add the NSID to the lexicon manifest instead", [path])
}

allowed(group) if group in shared

allowed(group) if group in lib.apps

# A tool's OAuth permission set sits beside the groups, as locallibraryPublishing does.
allowed(group) if {
	some app in lib.apps
	group == sprintf("%sPublishing", [app])
}
