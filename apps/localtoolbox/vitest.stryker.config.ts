// The suite as Stryker runs it (stryker.config.mjs): all of it, but for the
// two files that read the sources as text to check them against each other
// and the README, and the one that builds the extension and checks the bundle.
// In Stryker's sandbox those sources are its instrumented copies, which no
// longer say what the checks look for and build past the size budgets, and
// no mutant is anything those checks could kill: the build runs in a process of
// its own, where no mutant is ever switched on.
import { configDefaults, defineConfig, mergeConfig } from 'vitest/config';
import base from './vitest.config.ts';

export default mergeConfig(
	base,
	defineConfig({
		test: {
			exclude: [...configDefaults.exclude, 'test/build.test.ts', 'test/consistency.test.ts', 'test/docs.test.ts']
		}
	})
);
