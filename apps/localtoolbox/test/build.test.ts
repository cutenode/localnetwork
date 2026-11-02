// @vitest-environment node
//
// The thing that actually gets loaded into Chrome.
//
// test/consistency.test.ts reads vite.config.ts and asserts what it says it
// will do. This runs it and asserts what it did — which is the only way to
// catch a plugin that stopped copying, an asset path that went absolute (the
// popup is opened from chrome-extension://, where "/assets/x.js" is not where
// the file is), a sourcemap shipped by accident, or an `eval` pulled in by a
// dependency, which Manifest V3's content security policy refuses to run at
// all. None of those fail a unit test; all of them fail the extension.
import { describe, it, expect, beforeAll } from 'vitest';
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { resolve, join } from 'node:path';
import Ajv from 'ajv';
import blankSpace from 'ts-blank-space';

const ROOT = resolve(import.meta.dirname, '..');

const DIST = join(ROOT, 'dist');

const read = (path: string) => readFileSync(join(DIST, path), 'utf8');

const bytes = (path: string) => readFileSync(join(DIST, path));

const source = (path: string) => readFileSync(join(ROOT, path), 'utf8');

const there = (path: string) => existsSync(join(DIST, path));

const manifest = JSON.parse(source('src/manifest.json'));

// Everything under dist/, as paths relative to it.
const walk = (dir = DIST, prefix = ''): string[] =>
	readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
		entry.isDirectory() ? walk(join(dir, entry.name), `${prefix}${entry.name}/`) : [`${prefix}${entry.name}`]
	);

let files: string[];

beforeAll(() => {
	// The real build, run the way `pnpm build` runs it. Vitest sets NODE_ENV to `test`, which would
	// make Vite compile the components in Svelte's dev mode (file paths and all), so it is unset.
	const { NODE_ENV: _testMode, ...env } = process.env;
	execFileSync(process.execPath, [join(ROOT, 'node_modules/vite/bin/vite.js'), 'build'], {
		cwd: ROOT,
		env,
		stdio: 'pipe'
	});
	files = walk();
}, 120000);

describe('what the build produces', () => {
	it('is a folder Chrome can load: a manifest and everything it names', () => {
		expect(there('manifest.json')).toBe(true);
		expect(JSON.parse(read('manifest.json'))).toEqual(manifest);

		const [contentScript] = manifest.content_scripts;

		for (const file of [
			...contentScript.js,
			...contentScript.css,
			...Object.values(manifest.icons),
			manifest.action.default_popup,
			manifest.background.service_worker
		]) {
			expect(there(file), file).toBe(true);
		}
	});

	it('ships the page scripts as they are written, less their types', () => {
		// The page scripts are TypeScript with nothing to compile but the types:
		// what runs on soundcloud.com is the file in the repository with each
		// annotation blanked out, so every line and column is where it was.
		for (const name of ['content', 'downloaded-store', 'state-transfer']) {
			const shipped = read(`${name}.js`);
			expect(shipped, name).toBe(blankSpace(source(`${name}.ts`)));
			// Still a classic script: a content script cannot be a module, and the
			// shared two define page globals, which a module cannot do.
			expect(shipped, name).not.toMatch(/^\s*(import|export)\s/m);
		}

		expect(read('content.css')).toBe(source('content.css'));
	});

	it('holds nothing that is not meant to be there', () => {
		const top = readdirSync(DIST).sort();
		expect(top).toEqual(
			[
				'assets',
				'background.js',
				'content.css',
				'content.js',
				'downloaded-store.js',
				'icons',
				'manifest.json',
				'offscreen.html',
				'popup.html',
				'state-transfer.js'
			].sort()
		);
		// A sourcemap is several times the size of the bundle and ships the whole
		// source with it; the production build has none.
		expect(files.filter((file) => file.endsWith('.map'))).toEqual([]);

		for (const file of files.filter((f) => /\.(js|css|html)$/.test(f))) {
			expect(read(file), file).not.toContain('sourceMappingURL');
		}
	});

	it('stays small enough to be an extension rather than an application', () => {
		const total = files.reduce((sum, file) => sum + statSync(join(DIST, file)).size, 0);
		expect(total).toBeLessThan(5 * 1024 * 1024);
	});
});

describe('the popup page', () => {
	it('loads its assets by relative path, as a chrome-extension:// page must', () => {
		const html = read('popup.html');
		const references = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1]);
		expect(references.length).toBeGreaterThan(2);

		for (const reference of references) {
			expect(reference.startsWith('/'), reference).toBe(false);
			expect(/^[a-z]+:/i.test(reference), reference).toBe(false);
			expect(there(reference.replace(/^\.\//, '')), reference).toBe(true);
		}
	});

	it('loads the two shared page scripts before the bundle that reads them', () => {
		const html = read('popup.html');
		const scripts = [...html.matchAll(/<script[^>]*src="([^"]+)"/g)].map((m) => m[1]);
		expect(scripts.slice(0, 2)).toEqual(['./downloaded-store.js', './state-transfer.js']);
		expect(scripts.slice(2).every((src) => src.includes('assets/'))).toBe(true);
		// They define globals, which a module cannot do.
		expect(html).not.toMatch(/<script[^>]+type="module"[^>]+downloaded-store/);
	});

	it('has no inline script and pulls nothing off the network', () => {
		const html = read('popup.html');
		// Manifest V3 refuses both: inline script, and any remote code.
		expect(html).not.toMatch(/<script(?![^>]*\ssrc=)[^>]*>[\s\S]*?\S[\s\S]*?<\/script>/);
		expect(html).not.toMatch(/https?:\/\//);

		for (const file of files.filter((f) => f.endsWith('.css'))) {
			const css = read(file);

			for (const [, url] of css.matchAll(/url\(\s*['"]?([^'")]+)/g)) {
				expect(/^(https?:)?\/\//.test(url), `${file}: ${url}`).toBe(false);
			}
		}
	});

	// Chrome gives every extension page `body { font-family: system-ui;
	// font-size: 75% }`, unlayered, and an unlayered rule beats any layer: a
	// body rule in `@layer base` loses to it, and the popup draws in the system
	// font at 12px. So the type is set on body outside every layer.
	it('sets its own type on body where Chrome’s sheet for extension pages cannot beat it', () => {
		const css = files
			.filter((file) => file.endsWith('.css'))
			.map(read)
			.join('');

		const unlayered = outsideLayers(css);
		const body = [...unlayered.matchAll(/(?:^|[}\s])body\{([^}]*)\}/g)].map((m) => m[1]).join(';');
		expect(body).toMatch(/font-family:var\(--font-sans\)/);
		expect(body).toMatch(/font-size:var\(--text-label\)/);
	});
});

/** The stylesheet with every `@layer name { … }` block taken out. */
function outsideLayers(css: string) {
	let out = '';
	let depth = 0;

	for (let i = 0; i < css.length; i++) {
		if (depth === 0 && css.startsWith('@layer', i)) {
			const open = css.indexOf('{', i);
			const statement = css.indexOf(';', i);

			// `@layer a, b;` only declares the order, and has no block.
			if (statement !== -1 && statement < open) {
				i = statement;
				continue;
			}

			depth = 1;
			i = open;
			continue;
		}

		if (depth > 0) {
			if (css[i] === '{') depth++;
			else if (css[i] === '}') depth--;
			continue;
		}

		out += css[i];
	}

	return out;
}

describe('the bundle', () => {
	const js = () => files.filter((file) => /\.m?js$/.test(file)).map((file) => [file, read(file)]);

	// The popup is built from nothing every time the toolbar icon is clicked,
	// and in these tools that is constantly, so what has to compile before
	// anything can be drawn is kept to the shell: the menu, the registry's
	// names and colours, the stores and the ghost. Every panel is a chunk of
	// its own, fetched when its tool is opened, and so is each of the two
	// sheets. A stray static import of one undoes that silently — the extension
	// still works, it just opens slower every time — which is what this is here
	// to catch.
	it('boots from a chunk with the shell in it and none of the tools', () => {
		const entry = [...read('popup.html').matchAll(/<script[^>]*src="\.\/(assets\/[^"]+\.mjs)"/g)]
			.map((match) => match[1])
			.at(-1);

		// What has to arrive before anything is drawn is the entry and every
		// chunk it imports statically, not the entry alone: code the shell shares
		// with the panels is split into a chunk of its own, which the entry then
		// imports — the entry by itself is a quarter of what a boot reads.
		const boot = new Set<string>();

		const visit = (file: string) => {
			if (boot.has(file)) return;
			boot.add(file);

			for (const [, path] of read(file).matchAll(/(?:from|import)\s*"\.\/([^"]+\.mjs)"/g)) {
				visit(`assets/${path}`);
			}
		};

		visit(entry!);
		const size = [...boot].reduce((sum, file) => sum + read(file).length, 0);

		// About 105 KB. The room above it is for the shell to grow, not for a
		// sheet or a library: the settings sheet imported statically, on its own,
		// takes it to 290.
		expect(size, `${[...boot].join(', ')}: ${Math.round(size / 1024)} KB`).toBeLessThan(120 * 1024);

		// One chunk per panel, and one each for the two sheets, which are not
		// panels but bring the same component library with them.
		expect(files.filter((file) => /^assets\/Panel-[^/]+\.mjs$/.test(file))).toHaveLength(4);
		expect(files.filter((file) => /^assets\/QueueSheet-[^/]+\.mjs$/.test(file))).toHaveLength(1);
		expect(files.filter((file) => /^assets\/SettingsSheet-[^/]+\.mjs$/.test(file))).toHaveLength(1);

		// A small panel or sheet could slip under the budget above; nothing that
		// is only in one can be in what the popup boots from. Each marker has to
		// be in some other chunk, so a reworded one fails here rather than
		// passing on nothing.
		const chunks = js();

		for (const marker of ['api.droploud.com', 'checking download details', 'close the queue', 'close settings']) {
			expect(
				chunks.some(([file, code]) => !boot.has(file) && code.includes(marker)),
				`${marker} is in no chunk at all`
			).toBe(true);

			for (const file of boot) {
				expect(read(file), `${marker} is in ${file}, which the popup boots from`).not.toContain(marker);
			}
		}
	});

	// Vite's dynamic-import helper preloads a chunk's dependencies by putting
	// <link rel="modulepreload" crossorigin> in the head. Chrome counts a
	// crossorigin preload on a chrome-extension:// page as a different request
	// from the module fetch that follows it — "cross-world extension resource
	// mismatch" — so each one is fetched, thrown away and warned about, and
	// there is nothing to gain either way: these are files inside the
	// extension, not a network round trip worth saving. `modulePreload: false`
	// in vite.config.ts turns it off, leaving only the stylesheet a panel's
	// chunk genuinely has to load before it renders.
	it('preloads no script, which a chrome-extension:// page cannot use', () => {
		expect(read('popup.html')).not.toContain('modulepreload');

		const entry = [...read('popup.html').matchAll(/<script[^>]*src="\.\/(assets\/[^"]+\.mjs)"/g)]
			.map((match) => match[1])
			.at(-1);

		// The table the helper maps a dynamic import's dependencies through. It
		// is there as long as any chunk has one, which the panels' CSS gives it;
		// gone would mean Vite changed how it emits this, and the check below
		// would be quietly passing on nothing.
		const table = /m\.f=\[([^\]]*)\]/.exec(read(entry!));
		expect(table, 'no dependency table found — has Vite changed its codegen?').not.toBe(null);

		for (const [, dependency] of table![1].matchAll(/["'`]([^"'`]+)["'`]/g)) {
			expect(dependency, `${dependency} is preloaded, and only a stylesheet may be`).toMatch(/\.css$/);
		}
	});

	it('runs under the Manifest V3 content security policy', () => {
		for (const [file, code] of js()) {
			// eval and new Function are both blocked outright for an extension page.
			expect(code, file).not.toMatch(/[^.\w]eval\s*\(/);
			expect(code, file).not.toMatch(/new\s+Function\s*\(/);
			expect(code, file).not.toMatch(/document\.write\s*\(/);
		}
	});

	// Every built chunk, not just the one the page names: the panels are
	// chunks of their own now, and they are where most of the URLs are.
	it('asks for nothing off the network at runtime', () => {
		for (const [file, code] of js().filter(([name]) => name.startsWith('assets/'))) {
			// The only URLs the popup builds are SoundCloud's, and they are built
			// from strings in the source rather than fetched as code. The one
			// exception is a scheme put in front of an address read out of an
			// artist's bio (src/tools/artists/profiles.js), which is opened when
			// its badge is pressed and never fetched: the host is the bio's, not
			// the bundle's. Svelte's own errors end in a link to their page on
			// svelte.dev, which is text in a message, not a request. The settings
			// sheet's "report a bug" is a link to this repository's issue form
			// (src/lib/bug-report.ts), opened in a tab and never fetched.
			for (const [, url] of code.matchAll(/["'`](https?:\/\/[^"'`\s]+)/g)) {
				const isLinkOut =
					/^https:\/\/\$\{\w+\}$/.test(url) ||
					url.startsWith('https://svelte.dev/e/') ||
					url === 'https://github.com/cutenode/localnetwork/issues/new';

				if (isLinkOut) continue;
				expect(url, `${file}: ${url}`).toMatch(
					/soundcloud\.com|sndcdn\.com|beatport\.com|bandcamp\.com|zipdj\.com|mp3poolonline\.com|hypeddit|droploud|toneden|fanlink|laylo|feature\.fm|ffm\.to|pumpyoursound|gate\.sc|cloudfront\.net|w3\.org|json-schema\.org|dropbox|drive\.google/i
				);
			}
		}
	});

	it('uses no API that Manifest V2 had and V3 removed', () => {
		for (const [file, code] of js()) {
			for (const gone of [
				'chrome.extension.getBackgroundPage',
				'chrome.tabs.executeScript',
				'chrome.tabs.insertCSS',
				'chrome.browserAction',
				'chrome.pageAction',
				'webRequestBlocking'
			]) {
				expect(code, `${file}: ${gone}`).not.toContain(gone);
			}
		}
	});

	it('carries no path from the machine that built it', () => {
		for (const [file, code] of js()) {
			expect(code, file).not.toContain(ROOT);
			expect(code, file).not.toMatch(/\/Users\/[a-z]/i);
		}
	});
});

describe('the background worker', () => {
	// A service worker has no document, no window and no localStorage, and
	// Chrome refuses to start one that reaches for them as it loads. The walk
	// that needs a document runs in the offscreen page for exactly that reason
	// (background.ts), so a static import that pulled a panel's code, or a site
	// module, into the worker would be the bug this is here to catch.
	it('loads only files beside it, none of which reaches for a page', () => {
		const loaded = new Set<string>();

		const visit = (file: string) => {
			if (loaded.has(file)) return;
			loaded.add(file);

			for (const [, path] of read(file).matchAll(/(?:from|import)\s*"\.\/([^"]+\.mjs)"/g)) {
				visit(file.startsWith('assets/') ? `assets/${path}` : path);
			}
		};

		visit('background.js');
		expect(loaded.size).toBeGreaterThan(1);

		for (const file of loaded) {
			expect(read(file), file).not.toMatch(/\b(document|window|localStorage|DOMParser)\b/);
		}
	});

	it('opens an offscreen page that loads its script by relative path', () => {
		const html = read('offscreen.html');
		const [, script] = /<script[^>]*src="\.\/(assets\/[^"]+\.mjs)"/.exec(html) ?? [];

		expect(script && there(script), html).toBe(true);
	});
});

// ---------------------------------------------------------------------------

// A Manifest V3 schema, as far as this extension uses it. Chrome's own
// validation happens at load time, in a dialog, on someone else's machine;
// this is the same refusal, here.
const MV3_SCHEMA = {
	$schema: 'http://json-schema.org/draft-07/schema#',
	type: 'object',
	required: ['manifest_version', 'name', 'version'],
	additionalProperties: false,
	properties: {
		manifest_version: { const: 3 },
		name: { type: 'string', minLength: 1, maxLength: 75 },
		// Chrome's version: one to four dot-separated integers, each 0–65535.
		version: {
			type: 'string',
			pattern: '^(0|[1-9][0-9]{0,4})(\\.(0|[1-9][0-9]{0,4})){0,3}$'
		},
		description: { type: 'string', maxLength: 132 },
		icons: {
			type: 'object',
			propertyNames: { pattern: '^(16|32|48|64|96|128)$' },
			additionalProperties: { type: 'string', minLength: 1 },
			minProperties: 1
		},
		action: {
			type: 'object',
			additionalProperties: false,
			properties: {
				default_popup: { type: 'string' },
				default_title: { type: 'string' },
				default_icon: { type: ['string', 'object'] }
			}
		},
		permissions: {
			type: 'array',
			uniqueItems: true,
			items: {
				enum: [
					'activeTab',
					'alarms',
					'bookmarks',
					'clipboardRead',
					'clipboardWrite',
					'contextMenus',
					'cookies',
					'downloads',
					'history',
					'idle',
					'notifications',
					'offscreen',
					'scripting',
					'storage',
					'tabs',
					'unlimitedStorage',
					'webNavigation',
					'webRequest'
				]
			}
		},
		host_permissions: {
			type: 'array',
			uniqueItems: true,
			items: { type: 'string', pattern: '^(\\*|https?|file|ftp)://(\\*|(\\*\\.)?[^/*]+)/.*$' }
		},
		content_scripts: {
			type: 'array',
			minItems: 1,
			items: {
				type: 'object',
				required: ['matches'],
				additionalProperties: false,
				properties: {
					matches: {
						type: 'array',
						minItems: 1,
						items: { type: 'string', pattern: '^(\\*|https?|file|ftp)://(\\*|(\\*\\.)?[^/*]+)/.*$' }
					},
					js: { type: 'array', items: { type: 'string' } },
					css: { type: 'array', items: { type: 'string' } },
					run_at: { enum: ['document_start', 'document_end', 'document_idle'] },
					all_frames: { type: 'boolean' }
				}
			}
		},
		background: { type: 'object' },
		web_accessible_resources: { type: 'array' },
		content_security_policy: { type: 'object' },
		minimum_chrome_version: { type: 'string' }
	}
};

describe('the manifest itself', () => {
	it('is a valid Manifest V3 document', () => {
		const validate = new Ajv({ allErrors: true, strict: false }).compile(MV3_SCHEMA);
		const ok = validate(manifest);
		expect(validate.errors ?? [], JSON.stringify(validate.errors, null, 2)).toEqual([]);
		expect(ok).toBe(true);
	});

	it('declares the version the package does', () => {
		expect(manifest.version).toBe(JSON.parse(source('package.json')).version);
	});

	it('names icons that really are PNGs of the size they claim', () => {
		for (const [size, path] of Object.entries<string>(manifest.icons)) {
			const png = bytes(path);
			// The 8-byte signature, then IHDR: width and height as big-endian u32.
			expect([...png.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
			expect(png.readUInt32BE(16), `${path} width`).toBe(Number(size));
			expect(png.readUInt32BE(20), `${path} height`).toBe(Number(size));
		}
	});

	it('asks for no permission the code never uses', () => {
		const code = [
			read('content.js'),
			read('downloaded-store.js'),
			read('state-transfer.js'),
			read('background.js'),
			...files.filter((f) => /\.m?js$/.test(f) && f.startsWith('assets/')).map(read)
		].join('\n');

		const evidence = new Map([
			['activeTab', /tabs\.query|tabs\.sendMessage/],
			['clipboardWrite', /clipboard\.(writeText|write)/],
			['storage', /chrome\.storage/],
			['downloads', /chrome\.downloads/],
			['offscreen', /chrome\.offscreen\.createDocument/]
		]);

		for (const permission of manifest.permissions) {
			expect(evidence.has(permission), `no rule for "${permission}"`).toBe(true);
			expect(evidence.get(permission)?.test(code), `"${permission}" is never used`).toBe(true);
		}
	});
});
