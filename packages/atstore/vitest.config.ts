import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		include: ['test/**/*.test.ts'],
		// The render tests start Chromium.
		testTimeout: 30_000,
		coverage: {
			provider: 'v8',
			include: ['src/**/*.ts'],
			// The CLI and the tools' recipes build and run the tools themselves, so `pnpm atstore` is
			// their test; the types are only types.
			exclude: ['src/cli.ts', 'src/types.ts', 'src/listings/{index,web,locallibrary,localtoolbox}.ts'],
			thresholds: { lines: 99, branches: 97, functions: 97, statements: 98 }
		}
	}
});
