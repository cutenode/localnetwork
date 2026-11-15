// What the browser downloads: the production build, built the way `pnpm build` builds it.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { beforeAll, expect, it } from 'vitest';
import { measure } from '../scripts/bundle-sizes.ts';

const ROOT = resolve(import.meta.dirname, '..');

const CLIENT = join(ROOT, 'build/client');

let files: string[];

// No deadline (the 0 below). The build blocks this worker (execFileSync), so Vitest can't stop a hung
// one, only fail a slow one after it finishes. How long it takes follows the machine's load (about 10s
// on a quiet one, over 300s with other test runs going), and CI's job timeout still ends a hung one.
beforeAll(async () => {
	// Vitest sets NODE_ENV to `test`, which would make Vite compile the components in Svelte's dev
	// mode, so it is unset.
	const { NODE_ENV: _testMode, ...env } = process.env;
	execFileSync(process.execPath, [join(ROOT, 'node_modules/vite/bin/vite.js'), 'build'], {
		cwd: ROOT,
		env,
		stdio: 'pipe'
	});
	files = readdirSync(CLIENT, { recursive: true, withFileTypes: true })
		.filter((entry) => entry.isFile())
		.map((entry) => join(entry.parentPath, entry.name));
}, 0);

it('ships no sourcemaps to the browser', () => {
	// The server's stay in build/server, for its stack traces.
	expect(files.filter((file) => file.endsWith('.map'))).toEqual([]);

	for (const file of files.filter((name) => /\.(js|css)$/.test(name))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('sourceMappingURL');
	}
});

// The sizes are held to how much a pull request grows them (benchmarks/budgets/web.json, which CI
// checks with tools/budgets): here, that each is measured.
it('measures what the web budgets hold', async () => {
	const budget = JSON.parse(readFileSync(join(ROOT, '../../benchmarks/budgets/web.json'), 'utf8'));
	const sizes = await measure(ROOT);

	expect(Object.keys(sizes).toSorted()).toEqual(Object.keys(budget.metrics).toSorted());
	// The entry, the root layout and the root error page with what they import, about 171 KiB; the
	// largest page's own, /mixes/preview, about 142 KiB, most of it Zod; every page's CSS, about 98 KiB.
	expect(sizes.initial_js).toBeGreaterThan(100 * 1024);
	expect(sizes.page_js).toBeGreaterThan(50 * 1024);
	expect(sizes.css).toBeGreaterThan(50 * 1024);
});

it('keeps fonts as files, which the CSP allows', () => {
	// font-src is 'self' (vite.config.ts), so a font inlined as a data URL is blocked and logs an error.
	for (const file of files.filter((name) => name.endsWith('.css'))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('data:font');
	}
});
