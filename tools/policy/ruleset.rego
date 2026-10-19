# The "Protect Main" ruleset (.github/rulesets/protect-main.json) requires exactly the passed
# check of every gated workflow. tools/ruleset.sh applies the file, and repo-checks.yml checks
# that GitHub's copy matches it.
package ruleset

import data.lib
import data.workflows

required := {c.context |
	some rule in lib.file(".github/rulesets/protect-main.json").rules
	rule.type == "required_status_checks"
	some c in rule.parameters.required_status_checks
}

passed := {wf.jobs.passed.name | some _, wf in workflows.gated}

deny contains msg if {
	some name in (passed - required)
	msg := sprintf(".github/rulesets/protect-main.json: require %q, then apply it with tools/ruleset.sh", [name])
}

deny contains msg if {
	some name in (required - passed)
	msg := sprintf(".github/rulesets/protect-main.json: no workflow reports %q, so nothing can merge", [name])
}
