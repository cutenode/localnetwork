# Labels, the labeler and the issue templates know every app, and every file gets a label
# (.github/NEW_PROJECT.md, "Steps every app shares").
package labels

import data.lib

label_names := {l.name | some l in lib.items(".github/labels.yml")}

labeler := lib.file(".github/labeler.yml")

# How the issue forms' "Which tool?" dropdowns name each app.
display(app) := "Local Network (the website)" if app == "web"

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
	not labels_file(app, manifest)
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
	some app in lib.apps
	form := ".github/ISSUE_TEMPLATE/feature_request.yml"
	options := {o | some field in lib.file(form).body; field.id == "tool"; some o in field.attributes.options}
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
	labels_file(name, path)
}

# actions/labeler ORs a label's entries: a file gets the label when one of them matches it.
labels_file(name, path) if {
	some config in labeler[name]
	some entry in config["changed-files"]
	entry_matches(entry, path)
}

entry_matches(entry, path) if {
	some glob in lib.strings(entry["any-glob-to-any-file"])
	lib.matches(glob, path)
}

# For one file, "all globs" means it matches each glob and none of the negated ones.
entry_matches(entry, path) if {
	some key in {"all-globs-to-any-file", "all-globs-to-all-files"}
	globs := lib.strings(entry[key])
	count(globs) > 0
	every glob in globs {
		glob_holds(glob, path)
	}
}

glob_holds(glob, path) if {
	startswith(glob, "!")
	not lib.matches(substring(glob, 1, -1), path)
}

glob_holds(glob, path) if {
	not startswith(glob, "!")
	lib.matches(glob, path)
}
