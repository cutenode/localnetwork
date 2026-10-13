// What the browser downloads: the production build, built the way `pnpm build` builds it.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { beforeAll, expect, it } from 'vitest';

const ROOT = resolve(import.meta.dirname, '..');

let files: string[];

beforeAll(() => {
	// Vitest sets NODE_ENV to `test`, which would make Vite compile the components in Svelte's dev
	// mode, so it is unset.
	const { NODE_ENV: _testMode, ...env } = process.env;
	execFileSync(process.execPath, [join(ROOT, 'node_modules/vite/bin/vite.js'), 'build'], {
		cwd: ROOT,
		env,
		stdio: 'pipe'
	});
	files = readdirSync(join(ROOT, 'build/client'), { recursive: true, withFileTypes: true })
		.filter((entry) => entry.isFile())
		.map((entry) => join(entry.parentPath, entry.name));
}, 120_000);

const bytes = (extension: string) =>
	files.filter((file) => file.endsWith(extension)).reduce((sum, file) => sum + statSync(file).size, 0);

it('ships no sourcemaps to the browser', () => {
	// The server's stay in build/server, for its stack traces.
	expect(files.filter((file) => file.endsWith('.map'))).toEqual([]);

	for (const file of files.filter((name) => /\.(js|css)$/.test(name))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('sourceMappingURL');
	}
});

it('keeps the client bundle within its budget', () => {
	// Every page's code together, uncompressed: about 434 KB of JavaScript and 64 KB of CSS, a third
	// of that over the wire. The room above is for the app to grow; a library that needs more than
	// that is worth deciding on, not finding here later.
	expect(bytes('.js')).toBeLessThan(500 * 1024);
	expect(bytes('.css')).toBeLessThan(80 * 1024);
});
