// What the browser downloads: the production build, built the way `pnpm build` builds it.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { compile } from 'svelte/compiler';
import { beforeAll, expect, it } from 'vitest';
import { measure, readBundle } from '../scripts/bundle-sizes.ts';

const ROOT = resolve(import.meta.dirname, '..');

const CLIENT = join(ROOT, 'build/client');

let files: string[];

let initialLoad: Set<string>;

let routes: [id: string, files: Set<string>][];

let initialStyles: Set<string>;

let routeStyles: Set<string>[];

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

	({ initialLoad, routes, initialStyles, routeStyles } = await readBundle(ROOT));
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
	// The entry, the root layout and the root error page with what they import, about 173 KiB; the
	// largest page's own, /mixes/preview, about 142 KiB, most of it Zod; every page's CSS, about 99 KiB.
	expect(sizes.initial_js).toBeGreaterThan(100 * 1024);
	expect(sizes.page_js).toBeGreaterThan(50 * 1024);
	expect(sizes.css).toBeGreaterThan(50 * 1024);
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

it('keeps fonts as files, which the CSP allows', () => {
	// font-src is 'self' (vite.config.ts), so a font inlined as a data URL is blocked and logs an error.
	for (const file of files.filter((name) => name.endsWith('.css'))) {
		expect(readFileSync(file, 'utf8'), file).not.toContain('data:font');
	}
});

it("bundles every other authority's lexicon the manifest pins, from the installed set", () => {
	// Vite's import.meta.glob skips dot-directories, so a lexicon folder that starts with one builds as nothing.
	const server = join(ROOT, '.svelte-kit/output/server');

	const bundled = readdirSync(server, { recursive: true, withFileTypes: true })
		.filter((entry) => entry.isFile() && entry.name.endsWith('.js'))
		.map((entry) => readFileSync(join(entry.parentPath, entry.name), 'utf8'))
		.join('\n');

	const pinned: { lexicons: string[] } = JSON.parse(readFileSync(join(ROOT, '../../lexicons/network.json'), 'utf8'));

	expect(pinned.lexicons.filter((nsid) => !bundled.includes(`"${nsid}"`))).toEqual([]);
});
