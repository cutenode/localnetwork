// The check on `dist plan` in locallibrary-ci.yml's lint job: dist releases every package with a
// binary that doesn't opt out, so a new one would otherwise join locallibrary's releases (and its tag
// namespace) unnoticed. release-cli.ts runs it in verify:rust, in Node rather than a shell pipe so it
// runs the same under cmd.exe.

interface Plan {
	releases?: { app_name: string }[];
}

/** A line when dist's plan (`dist plan --output-format=json`) releases anything but locallibrary-cli. */
export const releaseProblems = (plan: string): string[] => {
	// SAFETY: dist's JSON plan lists what it releases under `releases`, each with its `app_name`.
	const apps = ((JSON.parse(plan) as Plan).releases ?? []).map(({ app_name }) => app_name);
	const onlyTheCli = apps.length === 1 && apps[0] === 'locallibrary-cli';

	return onlyTheCli
		? []
		: [
				`dist would release ${apps.join(', ') || 'nothing'}, not only locallibrary-cli: set dist = false under [package.metadata.dist] in the new package's Cargo.toml`
			];
};
