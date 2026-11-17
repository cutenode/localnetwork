// Commit subjects follow Conventional Commits, `type(scope): summary`. repo-checks.yml checks each
// pull request's commits with commitlint against @commitlint/config-conventional, less the limits
// this repository's own messages don't keep.
export default {
	extends: ['@commitlint/config-conventional'],
	rules: {
		// A subject says what changed in full, and some on main run past 150 characters.
		'header-max-length': [0],
		// Bodies are paragraphs, one line each, with links in them.
		'body-max-line-length': [0],
		'footer-max-line-length': [0]
	}
};
