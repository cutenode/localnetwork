# Every `run:` step runs in bash with pipefail. A step without a `shell` runs `bash -e` on Linux
# and macOS, where a command that fails into `| tee` or `| jq` still passes, and PowerShell on
# Windows; `shell: bash` runs `bash --noprofile --norc -eo pipefail` on all three.
package shell

import data.lib

# The workflow's default covers every step, so a new one can't miss it. A step that needs
# PowerShell says `shell: pwsh` itself.
deny contains msg if {
	some path in lib.paths(".github/workflows/*.yml")
	object.get(lib.file(path), ["defaults", "run", "shell"], null) != "bash"
	msg := sprintf("%s: add `shell: bash` under `defaults.run`, so every step runs with pipefail", [path])
}

# A composite action's steps don't take the calling workflow's defaults, so each one that runs a
# script names its shell.
deny contains msg if {
	some path in lib.paths(".github/actions/*/action.yml")
	some i, step in object.get(lib.file(path), ["runs", "steps"], [])
	step.run
	not step.shell
	msg := sprintf("%s: step %d runs a script without a `shell`; add `shell: bash`", [path, i + 1])
}
