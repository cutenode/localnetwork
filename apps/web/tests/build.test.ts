// What the browser downloads: the production build, built the way `pnpm build` builds it.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { compile } from 'svelte/compiler';
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

let initialStyles: Set<string>;

let routeStyles: Set<string>[];

// The stylesheets only edit mode's editor brings (src/lib/components/edit/editor.ts), which loads with import() once
// someone opens it. None until a page has edit mode.
let editorStyles: Set<string>;

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

	// A Set's loop also visits what is added to it during the loop.
	const withImports = (key: string) => {
		const keys = new Set([key]);

		for (const each of keys) for (const dep of chunks[each].imports ?? []) keys.add(dep);

		return [...keys];
	};

	const nodeChunks = (index: number) => {
		const key = nodes.get(`nodes/${index}`);

		if (!key) throw new Error(`Vite's manifest has no chunk for node ${index}`);

		return withImports(key);
	};

	const nodeScripts = (index: number) => nodeChunks(index).map((each) => chunks[each].file);

	// Vite links a chunk's CSS with it, statically imported or not.
	const styles = (keys: string[]) => keys.flatMap((each) => chunks[each].css ?? []);

	const nodeStyles = (index: number) => styles(nodeChunks(index));

	// Node 0 is the root layout, which every page renders, and node 1 the root error page, which the
	// client loads on every page before it hydrates.
	initialLoad = new Set([...manifest.client.imports, ...nodeScripts(0), ...nodeScripts(1)]);

	initialStyles = new Set([...nodeStyles(0), ...nodeStyles(1)]);

	routes = manifest.routes.flatMap(({ id, page }) => {
		if (!page) return [];

		const scripts = [...page.layouts, page.leaf].flatMap((index) => (index === undefined ? [] : nodeScripts(index)));

		return [[id, new Set(scripts.filter((file) => !initialLoad.has(file)))]];
	});

	routeStyles = manifest.routes.flatMap(({ page }) =>
		page
			? [new Set([...page.layouts, page.leaf].flatMap((index) => (index === undefined ? [] : nodeStyles(index))))]
			: []
	);

	const pageStyles = new Set([...initialStyles, ...routeStyles.flatMap((each) => [...each])]);

	const editor = Object.keys(chunks).find((key) => key.endsWith('src/lib/components/edit/editor.ts'));

	editorStyles = new Set(editor ? styles(withImports(editor)).filter((file) => !pageStyles.has(file)) : []);
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
// visitor downloads. The build's total isn't capped: it grows with every page added, though a
// visitor only loads the pages they open. The room above each is for the app to grow; a library
// that needs more than that is worth deciding on, not finding here later.

it('keeps what every page loads within its budget', () => {
	// The entry, the root layout and the root error page with everything they import: SvelteKit's and
	// Svelte's runtimes, devalue, and the small components pages share (vite.config.ts). About 169 KiB.
	expect(size(initialLoad)).toBeLessThan(181 * 1024);
});

it("keeps each page's own JavaScript within its budget", () => {
	// What a route loads beyond that. The largest is /mixes/preview at about 135 KiB, most of it Zod,
	// which checks a dropped report file; the next, /projects/[slug], about 49 KiB.
	expect(routes.length).toBeGreaterThan(0);

	for (const [id, scripts] of routes) expect(size(scripts), id).toBeLessThan(144 * 1024);
});

it('keeps edit mode out of what visitors load', () => {
	const has = (file: string, text: string) => readFileSync(join(CLIENT, file), 'utf8').includes(text);

	// The edit bar is drawn only for people who can edit a page, so it's never in what every page loads.
	for (const file of initialLoad) expect(has(file, 'data-edit-bar'), file).toBe(false);

	// Its editor (the leave question is in it) loads with import() once they open it, so no page loads it up front.
	for (const [id, scripts] of [['every page', initialLoad] as const, ...routes])
		for (const file of scripts) expect(has(file, 'Leave without saving?'), `${id}: ${file}`).toBe(false);
});

it("keeps edit mode's styles out of what visitors load", () => {
	const EDIT = join(ROOT, 'src/lib/components/edit');
	const read = (file: string) => readFileSync(file, 'utf8');

	const imports = (file: string) =>
		[...read(join(EDIT, file)).matchAll(/'\.\/([\w-]+\.(?:svelte|css))'/g)].map(([, name]) => name);

	// What only editing draws: the editor's chunk with everything of edit mode's it imports, and the fields and slots a
	// page draws in edit mode. The bar isn't here: the server draws it for the view too.
	const editOnly = new Set([...imports('editor.ts'), 'InlineText.svelte', 'AddSlot.svelte']);

	for (const file of editOnly) for (const name of imports(file)) editOnly.add(name);

	const editFiles = new Set([...editOnly].map((name) => join(EDIT, name)));

	const everyPage = [...initialStyles].map((file) => read(join(CLIENT, file))).join('\n');

	const pages = [...new Set([...initialStyles, ...routeStyles.flatMap((each) => [...each])])]
		.map((file) => read(join(CLIENT, file)))
		.join('\n');

	// Their own styles, marked by Svelte's scoping class for a component's and the attributes a stylesheet selects, so
	// they go with the editor's chunk (or aren't needed), never with a page.
	const markers = new Set<string>();

	for (const file of editFiles) {
		const css = file.endsWith('.css')
			? read(file)
			: (compile(read(file), { filename: file, css: 'external' }).css?.code ?? '');

		for (const [marker] of css.matchAll(/svelte-[a-z0-9]+|data-[a-z-]+/g)) markers.add(marker);
	}

	expect(markers.size).toBeGreaterThan(0);
	expect([...markers].filter((marker) => pages.includes(marker))).toEqual([]);

	// Tailwind writes a rule into the stylesheet every page loads for each class it finds in any of the site's files, so
	// a class only these use would be there for nobody else. Those go in the component's own styles instead.
	const words = (text: string) => text.split(/[\s'"`{}]+/);

	const elsewhere = new Set(
		readdirSync(join(ROOT, 'src'), { recursive: true, withFileTypes: true })
			.map((entry) => join(entry.parentPath, entry.name))
			.filter(
				(file) =>
					/\.(svelte|ts|js|css|html)$/.test(file) &&
					!/\.(test|spec|stories)\.|[\\/]lib[\\/]testing[\\/]/.test(file) &&
					!editFiles.has(file)
			)
			.flatMap((file) => words(read(file)))
	);

	const hasRule = (word: string) =>
		['{', ':', ',', '>', '[', '.', ')', ' '].some((end) =>
			everyPage.includes(`.${word.replace(/[^\w-]/g, '\\$&')}${end}`)
		);

	const theirsAlone = [...editFiles].flatMap((file) =>
		words(read(file)).filter((word) => !elsewhere.has(word) && hasRule(word))
	);

	expect(theirsAlone).toEqual([]);
});

it('keeps the CSS within its budget', () => {
	// What visitors download, every page's together: about 98 KB. Edit mode's editor brings its own, which only people who
	// open it load.
	expect(bytes('.css') - size(editorStyles)).toBeLessThan(100 * 1024);
});

it('keeps fonts as files, which the CSP allows', () => {
	// font-src is 'self' (vite.config.ts), so a font inlined as a data URL is blocked and logs an error.
	for (const file of files.filter((name) => name.endsWith('.css'))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('data:font');
	}
});
