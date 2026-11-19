package shell_test

import data.fixtures
import data.shell

labels := fixtures.file(".github/workflows/labels.yml")

action := fixtures.file(".github/actions/last-passed/action.yml")

with_labels(contents) := fixtures.with_file(".github/workflows/labels.yml", contents)

with_action(contents) := fixtures.with_file(".github/actions/last-passed/action.yml", contents)

default_msg := ".github/workflows/labels.yml: add `shell: bash` under `defaults.run`, so every step runs with pipefail"

test_fixture_passes if {
	count(shell.deny) == 0 with input as fixtures.repo
}

test_workflow_without_defaults if {
	shell.deny == {default_msg} with input as with_labels(json.remove(labels, ["defaults"]))
}

test_workflow_defaults_without_shell if {
	broken := json.patch(labels, [{"op": "replace", "path": "/defaults/run", "value": {"working-directory": "apps/site"}}])
	shell.deny == {default_msg} with input as with_labels(broken)
}

test_workflow_defaults_to_another_shell if {
	broken := json.patch(labels, [{"op": "replace", "path": "/defaults/run/shell", "value": "pwsh"}])
	shell.deny == {default_msg} with input as with_labels(broken)
}

test_action_step_without_shell if {
	broken := json.remove(action, ["runs/steps/1/shell"])
	shell.deny == {".github/actions/last-passed/action.yml: step 2 runs a script without a `shell`; add `shell: bash`"} with input as with_action(broken)
}

# A JavaScript or Docker action has no steps of its own.
test_action_without_steps if {
	count(shell.deny) == 0 with input as with_action({"runs": {"using": "node24", "main": "index.js"}})
}
