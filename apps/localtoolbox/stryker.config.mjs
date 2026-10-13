// Mutation testing: Stryker changes the logic one small edit at a time and
// reruns the tests that reach it, so a mutant that survives is behaviour no
// test pins down. Slow, so CI runs it weekly (localtoolbox-scheduled.yml).
export default {
	testRunner: 'vitest',
	// The runner is named rather than found: Stryker looks for its plugins beside
	// its own package, and pnpm installs the runner somewhere else.
	plugins: ['@stryker-mutator/vitest-runner', './stryker-load-time.mjs'],
	// Leaves two test files out (see that file). Stryker's own `testFiles` would
	// too, but it also switches every mutant on only after the modules loaded.
	vitest: { configFile: 'vitest.stryker.config.ts' },
	// The files vitest.config.ts holds to full coverage. The components are left
	// out for the reason they have a lower floor there: much of what they compile
	// to is Svelte's bookkeeping, not ours.
	mutate: ['content.ts', 'downloaded-store.ts', 'state-transfer.ts', 'src/lib/*.ts', 'src/tools/**/*.ts', '!**/*.d.ts'],
	// Code that runs as a module loads is skipped, both where Stryker can tell
	// from coverage alone and where it can't (see stryker-load-time.mjs).
	ignoreStatic: true,
	ignorers: ['load-time'],
	// Stryker gives a mutant's tests the time they took in the first run, plus
	// this. Its default of five seconds misses that each test file sets up a page
	// of its own, so a mutant reached by a few hundred tests across many files
	// was cut off and counted as killed without anything having failed.
	timeoutMS: 30000,
	reporters: ['html', 'clear-text', 'progress'],
	// Just under the score the suite reached when this was set up (91.5%), so
	// the run fails on a real drop rather than on a mutant or two. Raise it as
	// survivors are killed.
	thresholds: { break: 91 }
};
