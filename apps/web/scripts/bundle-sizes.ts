/**
 * What the browser downloads from a production build (`pnpm build`), in the numbers
 * benchmarks/budgets/web.json holds a pull request to: the JavaScript every page loads, the largest
 * page's own on top of that, and all the CSS. Uncompressed: a third of that goes over the wire.
 *
 *   node scripts/bundle-sizes.ts <app-root> <measured-dir>   writes <measured-dir>/web.json
 *
 * <app-root> is a built apps/web, of this checkout or another (a base the CI job built itself).
 */
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { Manifest } from 'vite';

/** web.json's metrics, in bytes. */
export type BundleSizes = { initial_js: number; page_js: number; css: number };

// The parts of SvelteKit's generated server manifest read here: the files it preloads for the
// app's entry on every page, and the layouts and page (node indexes) that make up each page route.
type ServerManifest = {
	client: { imports: string[] };
	routes: { id: string; page: { layouts: (number | undefined)[]; leaf: number } | null }[];
};

/** What pages load, as paths under build/client. */
type Bundle = {
	initialLoad: Set<string>;
	/** Each page route's scripts beyond initialLoad. */
	routes: [id: string, files: Set<string>][];
	initialStyles: Set<string>;
	routeStyles: Set<string>[];
	/**
	 * The stylesheets only edit mode's editor brings (src/lib/components/edit/editor.ts), which loads with import()
	 * once someone opens it. None until a page has edit mode.
	 */
	editorStyles: Set<string>;
};

export async function readBundle(root: string): Promise<Bundle> {
	// A route's scripts are its nodes' chunks and everything those import statically, which Vite's
	// manifest lists. SvelteKit preloads the same.
	const { manifest }: { manifest: ServerManifest } = await import(
		join(root, '.svelte-kit/output/server/manifest-full.js')
	);

	const chunks: Manifest = JSON.parse(
		readFileSync(join(root, '.svelte-kit/output/client/.vite/manifest.json'), 'utf8')
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
	const initialLoad = new Set([...manifest.client.imports, ...nodeScripts(0), ...nodeScripts(1)]);

	const initialStyles = new Set([...nodeStyles(0), ...nodeStyles(1)]);

	const routes = manifest.routes.flatMap(({ id, page }): Bundle['routes'] => {
		if (!page) return [];

		const scripts = [...page.layouts, page.leaf].flatMap((index) => (index === undefined ? [] : nodeScripts(index)));

		return [[id, new Set(scripts.filter((file) => !initialLoad.has(file)))]];
	});

	const routeStyles = manifest.routes.flatMap(({ page }) =>
		page
			? [new Set([...page.layouts, page.leaf].flatMap((index) => (index === undefined ? [] : nodeStyles(index))))]
			: []
	);

	const pageStyles = new Set([...initialStyles, ...routeStyles.flatMap((each) => [...each])]);

	const editor = Object.keys(chunks).find((key) => key.endsWith('src/lib/components/edit/editor.ts'));

	const editorStyles = new Set(editor ? styles(withImports(editor)).filter((file) => !pageStyles.has(file)) : []);

	return { initialLoad, routes, initialStyles, routeStyles, editorStyles };
}

export async function measure(root: string): Promise<BundleSizes> {
	const client = join(root, 'build/client');

	const { initialLoad, routes, editorStyles } = await readBundle(root);

	const size = (files: Set<string>) => [...files].reduce((sum, file) => sum + statSync(join(client, file)).size, 0);

	const allCss = readdirSync(client, { recursive: true, withFileTypes: true })
		.filter((entry) => entry.isFile() && entry.name.endsWith('.css'))
		.reduce((sum, entry) => sum + statSync(join(entry.parentPath, entry.name)).size, 0);

	return {
		initial_js: size(initialLoad),
		page_js: Math.max(0, ...routes.map(([, scripts]) => size(scripts))),
		// What visitors download. Edit mode's editor brings its own, which only people who open it load.
		css: allCss - size(editorStyles)
	};
}

if (import.meta.main) {
	const [root, out] = process.argv.slice(2);

	if (root === undefined || out === undefined)
		throw new Error('usage: node scripts/bundle-sizes.ts <app-root> <measured-dir>');

	mkdirSync(out, { recursive: true });
	writeFileSync(join(out, 'web.json'), `${JSON.stringify(await measure(resolve(root)), null, '\t')}\n`);
}
