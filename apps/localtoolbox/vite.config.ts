// Build for the extension's UI.
//
// The extension used to ship exactly what it ran: classic scripts, one HTML
// page per tool, no build step. The UI is now a Svelte app, so there is one,
// but only for the extension's own pages: the popup, and the background
// worker with the offscreen page it opens, all bundled into ES modules.
// Everything that runs *in the page* (content.ts and the store it shares with
// the panels) is still a classic script: it is written in TypeScript, and the
// only compiling it gets is its types blanked out, so the content script keeps
// loading the same two files the manifest has always named.
//
//   pnpm build           → dist/, the folder to load as an unpacked extension
//   pnpm dev             → the same, rebuilt on every change
//
// Paths are relative (`base: './'`) because the popup is opened from a
// chrome-extension:// origin, where absolute asset URLs would resolve
// against the extension root rather than the page.
import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import blankSpace from 'ts-blank-space';
import { execFileSync } from 'node:child_process';
import { cpSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;

const root = resolve(here, 'src');

// The two scripts the popup shares with the content script. They define page
// globals (TbxDownloaded, TbxState), which only a classic script can do, so
// they are injected as plain <script src> tags rather than imported: Vite
// leaves tags it injects itself alone, and `unbundled` below puts the files
// beside the page.
const classicScripts = ['downloaded-store.js', 'state-transfer.js'];

// What ships as written, because it does not run in the popup: the content
// script's stylesheet, the icons, and the manifest — which lives under src/
// rather than at the repo root on purpose. It names files that only exist once
// they are built (popup.html), so a manifest at the root would invite Chrome
// to load the source folder and then refuse it; there is nothing loadable
// there now.
//
// Keys are where a file lands in dist/, values where it comes from.
const unbundled = {
	'manifest.json': 'src/manifest.json',
	'content.css': 'content.css',
	icons: 'icons'
};

// The classic scripts: the content script and the two shared ones. A classic
// script has nothing to compile but its types, so ts-blank-space replaces each
// annotation with spaces and leaves every line and column where it was — an
// error on soundcloud.com points at the same place in the source, with no
// sourcemap to ship. Keyed the same way as `unbundled`.
const pageScripts = {
	'content.js': 'content.ts',
	'downloaded-store.js': 'downloaded-store.ts',
	'state-transfer.js': 'state-transfer.ts'
};

const shipUnbundled = (): Plugin => ({
	name: 'toolbox-unbundled-files',
	apply: 'build',
	// After Vite has written (and possibly emptied) the output directory.
	closeBundle() {
		const out = resolve(here, 'dist');

		for (const [to, from] of Object.entries(unbundled)) {
			cpSync(resolve(here, from), resolve(out, to), { recursive: true });
		}

		for (const [to, from] of Object.entries(pageScripts)) {
			writeFileSync(resolve(out, to), blankSpace(readFileSync(resolve(here, from), 'utf8')));
		}
	}
});

// Vite marks the entry's <script> and <link> `crossorigin`, which is right for
// a page served over HTTP and pointless here: on a chrome-extension:// page
// both tags are same-origin reads of files inside the extension, and the
// attribute turns them into CORS requests instead. It is the same attribute,
// on the same kind of tag, that made the modulepreload links unusable (see
// `modulePreload` below) — there is no reason to keep it on these two.
const dropCrossorigin = (): Plugin => ({
	name: 'toolbox-no-crossorigin',
	transformIndexHtml: {
		order: 'post',
		handler: (html: string) => html.replace(/ crossorigin(?=[ >])/g, '')
	}
});

// The commit the build was made from, which the settings sheet shows beside the
// version and puts into a bug report: the manifest's number alone says which
// release, and a build from source is usually somewhere between two. A copy of
// the source with no git (a zip) builds without one.
function commit() {
	try {
		return execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
			cwd: here,
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'ignore']
		}).trim();
	} catch (_e) {
		return '';
	}
}

// The popup's alone: the offscreen page reads neither global.
const injectClassicScripts = (): Plugin => ({
	name: 'toolbox-classic-scripts',
	transformIndexHtml(_html, { filename }) {
		if (!filename.endsWith('popup.html')) return undefined;

		return classicScripts.map((src) => ({
			tag: 'script',
			attrs: { src: `./${src}` },
			injectTo: 'head-prepend' as const
		}));
	}
});

export default defineConfig(({ mode }) => ({
	root,
	base: './',
	plugins: [
		tailwindcss(),
		svelte({ configFile: resolve(here, 'svelte.config.ts') }),
		injectClassicScripts(),
		dropCrossorigin(),
		shipUnbundled()
	],
	resolve: {
		alias: { $lib: resolve(root, 'lib') }
	},
	define: { 'import.meta.env.VITE_COMMIT': JSON.stringify(commit()) },
	build: {
		outDir: '../dist',
		// A release starts from an empty folder; a watch does not. `pnpm dev`
		// rebuilds on every keystroke, and this folder is loaded into Chrome as an
		// unpacked extension while that happens — Chrome reads its files from disk
		// on demand, so emptying it mid-session takes the extension out from under
		// the browser. Clicking the icon in that window opens nothing at all until
		// the files are back. Stale chunks accumulating across a watch costs
		// nothing: popup.html names the ones it wants, and the next release build
		// clears them out.
		emptyOutDir: mode !== 'development',
		// Vite would otherwise inject a <link rel="modulepreload" crossorigin>
		// for each of a dynamic import's dependencies. On a chrome-extension://
		// page Chrome treats a crossorigin preload as a different request from
		// the module fetch that follows ("cross-world extension resource
		// mismatch"), so every one of them is fetched, thrown away and warned
		// about in the console. There is nothing to preload *for*, either: the
		// panels' chunks are files inside the extension, read from disk rather
		// than fetched over a network, so discovering one a moment later costs
		// no round trip worth saving. Off, this also takes the preload helper
		// and its polyfill out of the chunk the popup boots from.
		modulePreload: false,
		// A sourcemap is several times the size of the bundle it explains, and
		// an extension carries its files around with it: `pnpm dev` builds one,
		// a release does not.
		sourcemap: mode === 'development',
		rollupOptions: {
			// The popup, and the two that walk a gate for the page's Download button:
			// the background worker and the offscreen page it opens (background.ts).
			input: {
				popup: resolve(root, 'popup.html'),
				offscreen: resolve(root, 'offscreen.html'),
				background: resolve(root, 'background.ts')
			},
			// ES modules, named as such. Chrome serves an extension's .mjs files as
			// JavaScript, which is all a module script needs. The worker keeps a name
			// without a hash, because the manifest has to name it.
			output: {
				entryFileNames: (chunk) => (chunk.name === 'background' ? 'background.js' : 'assets/[name]-[hash].mjs'),
				chunkFileNames: 'assets/[name]-[hash].mjs'
			}
		}
	}
}));
