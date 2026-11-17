# shellcheck shell=sh
# The git hooks lefthook installs source this before they start it (`rc` in lefthook.yml; `lefthook run` doesn't). It
# puts the Node in .node-version first on PATH, through fnm, so the hooks run CI's Node whatever the shell or git client
# has, and an install they start builds the native modules for it. Without fnm, or without that version installed, PATH stays as it is.
# shellcheck disable=SC2016 # the inner sh expands it, under fnm's PATH.
if command -v fnm > /dev/null 2>&1 &&
	node_bin=$(fnm exec --using "$(git rev-parse --show-toplevel)/.node-version" sh -c 'dirname "$(command -v node)"'); then
	PATH="$node_bin:$PATH"
	export PATH
fi
