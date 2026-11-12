# The "Protect Main" ruleset (.github/rulesets/protect-main.json) requires exactly the passed
# check of every gated workflow. tools/ruleset.sh applies the file, and repo-checks.yml checks
# that GitHub's copy matches it.
package ruleset

import data.lib
import data.workflows

rules := lib.file(".github/rulesets/protect-main.json").rules

rule_types contains rule.type if some rule in rules

required contains c.context if {
	some rule in rules
	rule.type == "required_status_checks"
	some c in rule.parameters.required_status_checks
}

passed contains wf.jobs.passed.name if some wf in workflows.gated

deny contains msg if {
	some name in (passed - required)
	msg := sprintf(".github/rulesets/protect-main.json: require %q, then apply it with tools/ruleset.sh", [name])
}

deny contains msg if {
	some name in (required - passed)
	msg := sprintf(".github/rulesets/protect-main.json: no workflow reports %q, so nothing can merge", [name])
}

# The merge queue waits for the required checks on each group's merge_group event, and takes the
# group out when one never reports.
deny contains msg if {
	"merge_queue" in rule_types
	some path, wf in workflows.gated
	not workflows.runs_on(workflows.triggers(wf), "merge_group")
	msg := sprintf("%s: add `on.merge_group`; the merge queue waits for %q, which never reports otherwise", [path, wf.jobs.passed.name])
}

# The code scanning rule waits for CodeQL's analysis of each group's commit too.
deny contains msg if {
	"merge_queue" in rule_types
	"code_scanning" in rule_types
	not codeql_on_merge_groups
	msg := ".github/workflows/codeql.yml: run it on merge_group, or call it from a workflow that does (as ci.yml does); the code scanning rule holds each merge queue group until CodeQL analyses its commit"
}

codeql_on_merge_groups if workflows.runs_on(workflows.triggers(lib.file(".github/workflows/codeql.yml")), "merge_group")

codeql_on_merge_groups if {
	some path in lib.paths(".github/workflows/*.yml")
	wf := lib.file(path)
	workflows.runs_on(workflows.triggers(wf), "merge_group")
	some job in wf.jobs
	job.uses == "./.github/workflows/codeql.yml"
}

# The code scanning rule also holds a commit until it has a result in every category main has, so
# CodeQL analyses every language each time: #347 couldn't join the queue when only the languages
# whose code changed ran.
deny contains msg if {
	"code_scanning" in rule_types
	some id, job in lib.file(".github/workflows/codeql.yml").jobs
	not is_array(job.strategy.matrix.language)
	msg := sprintf(".github/workflows/codeql.yml: list every language in job %q's matrix; the code scanning rule holds a commit until each category main has gets a result", [id])
}
