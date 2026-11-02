// What the browser downloads: the production build, built the way `pnpm build` builds it.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { Manifest } from 'vite';
import { beforeAll, expect, it } from 'vitest';

const ROOT = resolve(import.meta.dirname, '..');

const CLIENT = join(ROOT, 'build/client');

// The parts of SvelteKit's generated server manifest read here: the files it preloads for the
// app's entry on every page, and the layouts and page (node indexes) that make up each page route.
type ServerManifest = {
	client: { imports: string[] };
	routes: { id: string; page: { layouts: (number | undefined)[]; leaf: number } | null }[];
};

let files: string[];

let initialLoad: Set<string>;

let routes: [id: string, files: Set<string>][];

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

	// A route's scripts are its nodes' chunks and everything those import statically, which Vite's
	// manifest lists. SvelteKit preloads the same.
	const { manifest }: { manifest: ServerManifest } = await import(
		join(ROOT, '.svelte-kit/output/server/manifest-full.js')
	);

	const chunks: Manifest = JSON.parse(
		readFileSync(join(ROOT, '.svelte-kit/output/client/.vite/manifest.json'), 'utf8')
	);

	const nodes = new Map(Object.entries(chunks).map(([key, chunk]) => [chunk.name, key]));

	const nodeScripts = (index: number) => {
		const key = nodes.get(`nodes/${index}`);

		if (!key) throw new Error(`Vite's manifest has no chunk for node ${index}`);

		// A Set's loop also visits what is added to it during the loop.
		const keys = new Set([key]);

		for (const each of keys) for (const dep of chunks[each].imports ?? []) keys.add(dep);

		return [...keys].map((each) => chunks[each].file);
	};

	// Node 0 is the root layout, which every page renders, and node 1 the root error page, which the
	// client loads on every page before it hydrates.
	initialLoad = new Set([...manifest.client.imports, ...nodeScripts(0), ...nodeScripts(1)]);

	routes = manifest.routes.flatMap(({ id, page }) => {
		if (!page) return [];

		const scripts = [...page.layouts, page.leaf].flatMap((index) => (index === undefined ? [] : nodeScripts(index)));

		return [[id, new Set(scripts.filter((file) => !initialLoad.has(file)))]];
	});
}, 0);

const bytes = (extension: string) =>
	files.filter((file) => file.endsWith(extension)).reduce((sum, file) => sum + statSync(file).size, 0);

const size = (scripts: Set<string>) => [...scripts].reduce((sum, file) => sum + statSync(join(CLIENT, file)).size, 0);

it('ships no sourcemaps to the browser', () => {
	// The server's stay in build/server, for its stack traces.
	expect(files.filter((file) => file.endsWith('.map'))).toEqual([]);

	for (const file of files.filter((name) => /\.(js|css)$/.test(name))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('sourceMappingURL');
	}
});

// The budgets are on JavaScript uncompressed, a third of that over the wire, and count what a
// visitor downloads. The build's total, which this used to cap, grew with every page added, though
// a visitor only loads the pages they open. The room above each is for the app to grow; a library
// that needs more than that is worth deciding on, not finding here later.

it('keeps what every page loads within its budget', () => {
	// The entry, the root layout and the root error page with everything they import: SvelteKit's and
	// Svelte's runtimes, devalue, and the small components pages share (vite.config.ts). About 179 KiB.
	expect(size(initialLoad)).toBeLessThan(180 * 1024);
});

it("keeps each page's own JavaScript within its budget", () => {
	// What a route loads beyond that. The largest is /mixes/preview at about 135 KiB, most of it Zod,
	// which checks a dropped report file; the next, /projects/[slug], about 49 KiB.
	expect(routes.length).toBeGreaterThan(0);

	for (const [id, scripts] of routes) expect(size(scripts), id).toBeLessThan(144 * 1024);
});

it('keeps the CSS within its budget', () => {
	// Every page's together, about 90 KB.
	expect(bytes('.css')).toBeLessThan(100 * 1024);
});

it('keeps fonts as files, which the CSP allows', () => {
	// font-src is 'self' (vite.config.ts), so a font inlined as a data URL is blocked and logs an error.
	for (const file of files.filter((name) => name.endsWith('.css'))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('data:font');
	}
});
