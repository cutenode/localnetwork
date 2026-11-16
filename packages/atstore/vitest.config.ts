import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		// The render tests start Chromium.
		testTimeout: 30_000,
		// `pnpm test` runs `unit` and `pnpm test:sticks` runs `sticks`. The sticks' tests make disk images
		// with hdiutil and diskutil, which can stop answering for hours, so the pre-push hook leaves them out.
		projects: [
			{ extends: true, test: { name: 'unit', include: ['test/**/*.test.ts'], exclude: ['test/sticks.test.ts'] } },
			{ extends: true, test: { name: 'sticks', include: ['test/sticks.test.ts'] } }
		],
		coverage: {
			provider: 'v8',
			include: ['src/**/*.ts'],
			// The CLI and the tools' recipes build and run the tools themselves, so `pnpm atstore` is
			// their test; the sticks' tests need macOS, which CI isn't; the types are only types.
			exclude: ['src/cli.ts', 'src/sticks.ts', 'src/types.ts', 'src/listings/{index,web,locallibrary,localtoolbox}.ts'],
			thresholds: { lines: 100, branches: 100, functions: 100, statements: 100 }
		}
	}
});
