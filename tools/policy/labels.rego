# Labels, the labeler and the issue templates know every app, every file gets a label, and every
# code file an area (.github/NEW_PROJECT.md, "Steps every app shares").
package labels

import data.lib

label_names contains l.name if some l in lib.items(".github/labels.yml")

labeler := lib.file(".github/labeler.yml")

# How the issue forms' "Which tool?" dropdowns name each app.
display("web") := "Local Network (the website)"

display(app) := app if app != "web"

deny contains msg if {
	some name, _ in labeler
	not name in label_names
	msg := sprintf(".github/labeler.yml: %q isn't in .github/labels.yml", [name])
}

deny contains msg if {
	some app in lib.apps
	not app in label_names
	msg := sprintf(".github/labels.yml: add a label for %s", [app])
}

deny contains msg if {
	some app in lib.apps
	some manifest in lib.paths(sprintf("apps/%s/*", [app]))
	not applies(app, manifest)
	msg := sprintf(".github/labeler.yml: the %q label should match %s", [app, manifest])
}

deny contains msg if {
	some app in lib.apps
	template := lib.file(sprintf(".github/ISSUE_TEMPLATE/%s-bug.yml", [app]))
	some label in ({"bug", app} - {l | some l in template.labels})
	msg := sprintf(".github/ISSUE_TEMPLATE/%s-bug.yml: add the %q label", [app, label])
}

deny contains msg if {
	some app in lib.apps
	not lib.file(sprintf(".github/ISSUE_TEMPLATE/%s-bug.yml", [app]))
	msg := sprintf(".github/ISSUE_TEMPLATE: add %s-bug.yml", [app])
}

deny contains msg if {
	form := ".github/ISSUE_TEMPLATE/feature_request.yml"
	options := {o | some field in lib.file(form).body; field.id == "tool"; some o in field.attributes.options}
	some app in lib.apps
	not display(app) in options
	msg := sprintf("%s: add %q to the \"Which tool?\" options", [form, display(app)])
}

# Every file the repository tracks gets at least one label (tools/policy.sh lists them).
deny contains msg if {
	some path in lib.tracked_files
	not labeled(path)
	msg := sprintf("%s: no label in .github/labeler.yml matches it", [path])
}

labeled(path) if {
	some name, _ in labeler
	applies(name, path)
}

# An area is a "<tool>: <area>" label, and the labeler adds each one.
area(name) if contains(name, ": ")

deny contains msg if {
	some name in label_names
	area(name)
	not labeler[name]
	msg := sprintf(".github/labeler.yml: add a rule for the %q label", [name])
}

# Every code file gets an area, so a pull request says which part of a tool it changes. The
# test helpers every area shares are left out.
code_folders := {"apps/web/src/", "apps/locallibrary/src/", "apps/localtoolbox/src/", "apps/localtoolbox/test/", "crates/", "plugins/"}

arealess := {"apps/web/src/lib/testing/**", "apps/web/src/writing.test.ts"}

deny contains msg if {
	some path in lib.tracked_files
	some folder in code_folders
	startswith(path, folder)
	not arealess_file(path)
	not in_area(path)
	msg := sprintf("%s: no area label in .github/labeler.yml matches it", [path])
}

arealess_file(path) if {
	some pattern in arealess
	lib.matches(pattern, path)
}

in_area(path) if {
	some name, _ in labeler
	area(name)
	applies(name, path)
}

# Whether label `name` applies to `path`. actions/labeler ORs a label's entries: a file gets the
# label when one of them matches it.
applies(name, path) if {
	some config in labeler[name]
	some entry in config["changed-files"]
	entry_matches(entry, path)
}

# For one file, "any glob to all files" is the same as "any glob to any file".
entry_matches(entry, path) if {
	some key in {"any-glob-to-any-file", "any-glob-to-all-files"}
	some pattern in lib.string_set(entry[key])
	lib.matches(pattern, path)
}

# For one file, "all globs" means it matches each glob and none of the negated ones.
entry_matches(entry, path) if {
	some key in {"all-globs-to-any-file", "all-globs-to-all-files"}
	globs := lib.string_set(entry[key])
	count(globs) > 0
	every pattern in globs {
		glob_holds(pattern, path)
	}
}

glob_holds(pattern, path) if {
	startswith(pattern, "!")
	not lib.matches(substring(pattern, 1, -1), path)
}

glob_holds(pattern, path) if {
	not startswith(pattern, "!")
	lib.matches(pattern, path)
}
