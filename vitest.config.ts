// Tests for the repository's own tooling in tools/, run from the root with `pnpm test:tools`.
import { defineConfig } from 'vitest/config';
import { resolve } from 'node:path';

export default defineConfig({
	test: {
		// Its own module cache, so this run and the apps' runs don't share the workspace root's.
		fsModuleCachePath: resolve(import.meta.dirname, 'node_modules/.vitest-tools-cache'),
		include: ['tools/**/*.test.ts'],
		coverage: {
			provider: 'v8',
			reporter: ['text', 'json-summary'],
			thresholds: {
				'!tools/oxlint/**': { perFile: true, statements: 100, branches: 100, functions: 100, lines: 100 },
				// The vendored anti-slop rules unwrap parentheses Oxlint's parser never keeps and guard AST
				// shapes it never produces, so their tests can't reach every line. A glob can't loosen the
				// top-level thresholds, which cover every file, so both sets are globs. Held just under the
				// plugin's current coverage.
				'tools/oxlint/**': { statements: 91, branches: 88, functions: 99, lines: 96 }
			}
		}
	}
});
