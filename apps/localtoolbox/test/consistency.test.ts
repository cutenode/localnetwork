// Cross-file agreement: the manifest, the build, the tool registry and the
// stylesheets must refer to each other correctly.
//
// The popup is compiled now, so a broken import is a build error rather than a
// silent misfire — but the seams the compiler cannot see are still here: what
// the manifest names and the build copies, what runs in the page as a classic
// script, the Tailwind variants shadcn-svelte's components rely on, and the
// storage keys the exported state file is written under.
import { describe, it, expect } from 'vitest';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { HOSTS, SITES, type Site } from '../src/tools/third-party-downloads/sites/index.ts';

const ROOT = resolve(import.meta.dirname, '..');

const read = (path: string) => readFileSync(resolve(ROOT, path), 'utf8');

const exists = (path: string) => existsSync(resolve(ROOT, path));

// Under src/, not at the repo root: see "the build" below.
const manifest = JSON.parse(read('src/manifest.json'));

const viteConfig = read('vite.config.ts');

const appCss = read('src/app.css');

describe('manifest.json', () => {
	it('is a Manifest V3 extension with only the permissions the README lists', () => {
		expect(manifest.manifest_version).toBe(3);
		expect(manifest.permissions).toEqual(['activeTab', 'clipboardWrite', 'storage', 'downloads']);
		expect(manifest.background).toBeUndefined();
	});

	// The Downloader reads gate pages from the panel, so every site it knows
	// needs a host permission; nothing reaches beyond its content script's own
	// match pattern otherwise. A pattern here that no site claims is a hole, and
	// a site whose host is not covered is a tool that cannot fetch anything.
	it('reaches exactly the sites the Downloader walks', async () => {
		const covers = (host: string) =>
			manifest.host_permissions.some((pattern: string) => {
				const [, allowed] = /^https:\/\/(.+)\/\*$/.exec(pattern) ?? [];

				return allowed === host || (allowed?.startsWith('*.') && host.endsWith(allowed.slice(2)));
			});

		// The registry itself rather than the text of its modules: one module can
		// export more than one site (toneden.js is ToneDen and FanLink), and a
		// reading of its source that stopped at the first `hosts:` never saw the
		// second.
		expect(HOSTS.length).toBeGreaterThan(0);

		for (const host of HOSTS) expect(covers(host), host).toBe(true);

		// An alias is a host a site claims but rewrites before fetching, so a
		// permission for it would be one the extension never uses.
		for (const alias of SITES.flatMap((site) => site.aliases ?? [])) {
			expect(covers(alias), alias).toBe(false);
		}

		// Every site any module under sites/ exports is registered, so every host
		// it claims is one the panel can actually match a link to.
		const folder = resolve(ROOT, 'src/tools/third-party-downloads/sites');

		for (const file of readdirSync(folder).filter((name) => name !== 'index.ts')) {
			const sites = Object.values(await import(resolve(folder, file))).filter(
				(value): value is Site => value instanceof Object && 'hosts' in value && Array.isArray(value.hosts)
			);

			expect(sites.length, `${file} exports no site`).toBeGreaterThan(0);

			for (const site of sites) expect(SITES, `${file}: ${site.id}`).toContain(site);
		}
	});

	it('refers only to files that ship, with the store loaded before the content script', () => {
		const [contentScript] = manifest.content_scripts;
		expect(contentScript.matches).toEqual(['https://soundcloud.com/*']);
		expect(contentScript.js).toEqual(['downloaded-store.js', 'content.js']);
		// The scripts are written in TypeScript and shipped with their types
		// blanked out (see "the build" below), so it is the .ts that has to exist.
		const sources = contentScript.js.map((file: string) => file.replace(/\.js$/, '.ts'));

		for (const file of [...sources, ...contentScript.css, ...Object.values(manifest.icons)]) {
			expect(exists(file), file).toBe(true);
		}

		// The popup is built rather than shipped as written, so it is the build's
		// entry point that has to exist.
		expect(manifest.action.default_popup).toBe('popup.html');
		expect(exists('src/popup.html')).toBe(true);
	});
});

describe('the build', () => {
	// A map in vite.config.ts, as {where it lands: where it comes from}:
	// `unbundled` is copied as it is, `pageScripts` has its types blanked out.
	const mapIn = (name: string) => {
		const source = new RegExp(`const ${name} = \\{([\\s\\S]*?)\\n\\};`).exec(viteConfig)![1];
		const pairs = [...source.matchAll(/^\s+'?([\w.-]+)'?: '([^']+)',?$/gm)];

		return Object.fromEntries(pairs.map((m) => [m[1], m[2]]));
	};

	const unbundled = () => mapIn('unbundled');
	const pageScripts = () => mapIn('pageScripts');

	const classicScripts = () =>
		[.../const classicScripts = \[([^\]]+)\]/.exec(viteConfig)![1].matchAll(/'([^']+)'/g)].map((m) => m[1]);

	it('puts everything the manifest names beside the bundle', () => {
		// What runs in the SoundCloud page, plus the icons and the manifest
		// itself: none of it goes through the bundler, so the build has to put it
		// beside the compiled popup, under the name the manifest calls it by.
		const copied = { ...unbundled(), ...pageScripts() };
		const [contentScript] = manifest.content_scripts;

		for (const file of ['manifest.json', 'icons', ...contentScript.js, ...contentScript.css]) {
			expect(Object.keys(copied), file).toContain(file);
		}

		for (const source of Object.values(copied)) expect(exists(source), source).toBe(true);

		// The popup's classic scripts are page scripts too: the same files.
		for (const file of classicScripts()) expect(Object.keys(pageScripts()), file).toContain(file);
	});

	// A manifest at the repo root would invite Chrome to load the source folder,
	// where the popup it names does not exist until the build has run — which is
	// exactly the confusing half-failure ("the default_popup file doesn't
	// exist") that moving it under src/ prevents.
	it('keeps the repo root unloadable as an extension', () => {
		expect(exists('manifest.json')).toBe(false);
		expect(unbundled()['manifest.json']).toBe('src/manifest.json');
	});

	it('loads the shared classic scripts into the popup page', () => {
		// downloaded-store.js and state-transfer.js define page globals, which
		// only a classic script can do; the build injects them, and src/lib
		// reaches them by name (see globals.js).
		const classic = /const classicScripts = \[([^\]]+)\]/.exec(viteConfig)![1];
		expect(classic).toContain('downloaded-store.js');
		expect(classic).toContain('state-transfer.js');
		const globals = read('src/lib/globals.ts');
		expect(globals).toContain('TbxDownloaded');
		expect(globals).toContain('TbxState');
		// A module import would give each its own copy, so there must not be one.
		expect(globals).not.toMatch(/^import .*downloaded-store/m);
	});
});

describe('the tools', () => {
	const registry = read('src/lib/tools.ts');
	const ids = [...registry.matchAll(/^\s+id: '([^']+)',?$/gm)].map((m) => m[1]);
	const folders = readdirSync(resolve(ROOT, 'src/tools'));

	it('offers exactly the tools that have a panel', () => {
		expect(ids).toHaveLength(4);
		expect([...ids].sort()).toEqual([...folders].sort());

		for (const id of ids) expect(exists(`src/tools/${id}/Panel.svelte`), id).toBe(true);
	});

	it('gives each tool a name, a description, an icon and its own colour', () => {
		for (const id of ids) {
			const entry = registry.slice(registry.indexOf(`id: '${id}'`));
			const upTo = entry.slice(0, entry.indexOf('},'));
			expect(upTo, `${id} name`).toMatch(/name: '[^']+'/);
			expect(upTo, `${id} description`).toMatch(/description: '[^']+\.'/);
			expect(upTo, `${id} icon`).toMatch(/icon: \w+/);
			expect(upTo, `${id} brand`).toMatch(/brand: 'oklch\([^']+\)'/);
		}
	});

	it('paints the popup in the open tool’s colour, and derives the rest from it', () => {
		expect(registry).toContain("documentElement.style.setProperty('--brand'");

		for (const derived of ['--primary', '--ring']) {
			expect(appCss, derived).toContain(derived);
		}

		expect(appCss).toMatch(/--primary: color-mix\(in oklch, var\(--brand\)/);
	});
});

describe('the session queue', () => {
	const queue = read('src/lib/queue.svelte.ts');

	// A working list, not a library: session storage is what the browser throws
	// away when it closes. The marks, which do last, stay in local storage. The
	// place the popup was left in (view.svelte.js) is the only other thing kept
	// per session, for the same reason — where someone is in a piece of work,
	// not a setting.
	it('is kept in session storage, with the popup’s place and nothing else', () => {
		const view = read('src/lib/view.svelte.ts');

		for (const source of [queue, view]) {
			expect(source).toContain('chrome.storage.session');
			expect(source).not.toContain('chrome.storage.local');
		}

		expect(queue).toContain("const KEY = 'queue';");
		expect(view).toContain("const KEY = 'view';");
		expect(read('downloaded-store.ts')).toContain('chrome.storage.local');
		// The exported state file carries the queue but does not reach for it:
		// the marks and the details it reads out of local storage itself, while
		// the queue comes from the store above and goes back into it (Panel.svelte
		// hands `queue.entries` out and `doc.queue` back). That is what puts a
		// queue this store could only keep in memory into the file too.
		const transfer = read('state-transfer.ts');
		expect(transfer).toContain('chrome.storage.local');
		expect(transfer).not.toContain('chrome.storage.session');
	});

	// The content script paints a "Queued" pill from that same queue and offers
	// the button that fills it, and a content script is an untrusted context:
	// session storage is shut to one until a trusted context opens it, which is
	// a thing only the popup can do. Both halves have to be here — the door
	// opened, and the key it is opened for agreeing with the key the queue is
	// written under.
	it('is opened to the content script by the popup, and used there under the same key', () => {
		const content = read('content.ts');
		expect(queue).toContain('TRUSTED_AND_UNTRUSTED_CONTEXTS');
		expect(content).toContain("const QUEUE_KEY = 'queue';");
		expect(content).toContain('chrome.storage.session.get(QUEUE_KEY)');
		expect(content).toContain('chrome.storage.session.set({ [QUEUE_KEY]: next })');
		// Two files name that key — the store that owns it and the page that
		// paints from it — and a rename in one would quietly leave the other
		// reading an empty queue. The third place the queue turns up, the
		// exported state file, names no key at all: it goes through the store.
		expect(read('state-transfer.ts')).not.toContain('QUEUE_KEY');
		// The queue button writes the queue back whole; nothing on the page wipes
		// the area or opens it, which is the popup's alone to do.
		expect(content).not.toMatch(/storage\.session\.(remove|clear)\b/);
		expect(content).not.toContain('setAccessLevel');
	});

	// Two ways of remembering a value, and a panel has to reach for the right
	// one: an option lasts (localStorage, and it is in the exported state file),
	// a place lasts until the browser closes.
	it('is not where the panels’ remembered options live', () => {
		expect(read('src/lib/prefs.svelte.ts')).not.toContain('chrome.storage');
		const view = read('src/lib/view.svelte.ts');
		// What the panels kept is in the session and only in the session. The one
		// thing view.svelte.js does put in localStorage is which tool was open —
		// a mirror it reads synchronously so the popup has a route to draw before
		// the session answers — and that is written from `open`, nowhere else.
		expect(view).toContain("const HINT = 'view:open'");

		for (const [, call] of view.matchAll(/localStorage\.\w+\(([^)]*)\)/g)) {
			expect(call.trim(), call).toMatch(/^HINT/);
		}

		// The queue keeps a mirror of its own, for the same reason and under the
		// same rule: a copy of what the session holds, read synchronously so the
		// count in every header is there on the popup's first turn. It is written
		// from `MIRROR` and from nothing else, and it is no more a pref than the
		// route is — the session is still what settles both.
		const queueStore = read('src/lib/queue.svelte.ts');
		expect(queueStore).toContain("const MIRROR = 'queue:items'");

		for (const [, call] of queueStore.matchAll(/localStorage\.\w+\(([^)]*)\)/g)) {
			expect(call.trim(), call).toMatch(/^MIRROR/);
		}
	});

	// One remembered choice, shared: the sheet sets it when it hands a batch on,
	// and the tool it opens has to be reading the same key.
	it('is chosen as a source through the one shared preference', () => {
		expect(read('src/lib/source.ts')).toContain("export const SOURCE_PREF = 'pref:source';");

		for (const id of ['soundcloud-playlist-to-text', 'third-party-downloads']) {
			expect(read(`src/tools/${id}/Panel.svelte`), id).toContain("from '$lib/source.ts'");
		}

		expect(read('src/lib/components/QueueSheet.svelte')).toContain("from '$lib/source.ts'");
	});

	// It is not a tool and must not creep back into being one: the registry is
	// what the menu lists, and the queue is reached from the header of every
	// view instead. Both halves of that are checked — nothing under src/tools/
	// for it, and a button in the shell that opens the sheet.
	it('is in the header of every view rather than in the tool menu', () => {
		expect(read('src/lib/tools.ts')).not.toContain("id: 'session-queue'");
		expect(exists('src/tools/session-queue')).toBe(false);

		for (const file of [
			'src/lib/components/ToolMenu.svelte',
			'src/lib/components/PanelShell.svelte',
			'src/lib/components/PanelGhost.svelte'
		]) {
			expect(read(file), file).toContain('QueueButton');
		}

		// The sheet is drawn by the shell, over whichever tool is open, and its
		// chunk is fetched the way a panel's is.
		expect(read('src/App.svelte')).toContain('loadQueueSheet');
		expect(read('src/lib/queue-sheet.ts')).toContain("import('./components/QueueSheet.svelte')");
	});
});

// The one kind of value the popup keeps that is not the popup's own: a
// setting the content script has to obey. It cannot live where the panels'
// options live, because a content script cannot see the popup's localStorage
// — so the two halves share a key in chrome.storage.local, and a default
// neither can import from the other.
describe('the settings', () => {
	const store = read('src/lib/settings.svelte.ts');
	const content = read('content.ts');

	it('are kept where the content script can read them, under one agreed key', () => {
		expect(store).toContain("const KEY = 'settings';");
		expect(store).toContain('chrome.storage.local');
		// And not where a panel's options go, which the page cannot see. (The
		// file says so in prose as well; this is the code.)
		expect(store).not.toMatch(/^(?!\s*(\/\/|\*)).*localStorage/m);
		expect(content).toContain("const SETTINGS_KEY = 'settings';");
	});

	it('have the same defaults on both sides of that key', () => {
		// Every setting is a flag, written down twice — once for the popup and
		// once for the page — and the two lists have to be the same list, in
		// every name and every default. A setting added to one side only is a
		// switch the other side never obeys.
		const flags = (source: string, declaration: string) => {
			// Typed or not, the declaration is found by its name; the flags are what is compared.
			const body = new RegExp(`${declaration}(?:: [^=\\n]+)? = \\{([\\s\\S]*?)\\};`).exec(source)![1];

			return [...body.matchAll(/^\s+(\w+): (true|false),?$/gm)].map((m) => [m[1], m[2]]);
		};

		const popup = flags(store, 'export const DEFAULTS');
		expect(popup).toEqual([
			['replaceDownloadButtons', 'true'],
			['likesListView', 'false']
		]);
		expect(flags(content, 'const SETTINGS_DEFAULTS')).toEqual(popup);
	});

	// The other kind of setting the sheet holds: the Downloader's, which stay in
	// the popup's own localStorage because nothing outside the popup reads them.
	// What matters here is that there is one of each — the sheet writes the
	// address and the panel reads it, and two `textPref()` calls over the one key
	// would be two copies of the value, leaving the panel to answer a gate with
	// the empty one it read as it mounted.
	it('let the Downloader’s be made once, where both the sheet and the panel read them', () => {
		const shared = read('src/lib/downloader.svelte.ts');
		expect(shared).toContain("textPref('pref:email')");
		expect(shared).toContain("textPref('pref:name')");
		expect(shared).toContain("flagPref('pref:mark', true)");
		// In prose it says why it is not in chrome.storage; in code it must not be.
		expect(shared).not.toMatch(/^(?!\s*(\/\/|\*)).*chrome\.storage/m);

		for (const file of ['src/lib/components/SettingsSheet.svelte', 'src/tools/third-party-downloads/Panel.svelte']) {
			expect(read(file), file).toContain("from '$lib/downloader.svelte.ts'");
			expect(read(file), file).not.toContain("'pref:email'");
		}

		// And they are set in the sheet rather than on the panel: the panel has no
		// field for either, only the button that opens the sheet.
		const panel = read('src/tools/third-party-downloads/Panel.svelte');
		expect(panel).toContain('view.settingsOpen = true');
		expect(panel).not.toContain('id="email"');
		expect(read('src/lib/components/SettingsSheet.svelte')).toContain('id="email"');
	});

	// The search badges are the same arrangement as the Downloader's options —
	// thrown in the sheet, read by a panel under it — over a list that is data
	// rather than code. Neither the sheet nor the panel may name a site: adding
	// one is an entry in the JSON file and nothing else, and a site written into
	// a component is one that file cannot switch off.
	it('let the search badges be a file, read by both the sheet and the panel', () => {
		const shared = read('src/lib/search-sites.svelte.ts');
		expect(shared).toContain("from './search-sites.ts'");
		expect(shared).toContain('flagPref(searchPrefKey(site.id), site.on)');
		expect(read('src/lib/search-sites.ts')).toContain("import catalogue from './search-sites.json'");
		// In prose it says why these are not in chrome.storage; in code they must
		// not be, for the reason the Downloader's are not.
		expect(shared).not.toMatch(/^(?!\s*(\/\/|\*)).*chrome\.storage/m);

		const sheet = read('src/lib/components/SettingsSheet.svelte');
		const panel = read('src/tools/manager/Panel.svelte');

		for (const [file, source] of [
			['the sheet', sheet],
			['the panel', panel]
		]) {
			expect(source, file).toContain("from '$lib/search-sites.svelte.ts'");

			for (const site of JSON.parse(read('src/lib/search-sites.json'))) {
				expect(source, `${file} names ${site.name}`).not.toContain(site.name);
			}
		}

		// Set in the sheet, and only there: the panel draws them and never says
		// which are on.
		expect(sheet).toContain('searchSites.set(');
		expect(panel).not.toContain('searchSites.set(');
	});

	// Title case is the third of these arrangements, over a transform that is a
	// file of its own: the switch made once, the words done in one place, and
	// the opened row the only thing that draws them that way — the list itself,
	// the clipboard's own copy of a title elsewhere and every export go on
	// saying what SoundCloud said.
	it('let the title casing be made once, over one transform', () => {
		const shared = read('src/lib/title-case.svelte.ts');
		expect(shared).toContain("flagPref('pref:title-case', false)");
		// In prose it says why it is not in chrome.storage; in code it must not
		// be, for the reason the Downloader's options are not.
		expect(shared).not.toMatch(/^(?!\s*(\/\/|\*)).*chrome\.storage/m);
		// Nor in the exported state file, whose keys are listed in one place.
		expect(read('state-transfer.ts')).not.toContain('pref:title-case');

		const sheet = read('src/lib/components/SettingsSheet.svelte');
		const links = read('src/tools/manager/links.ts');
		expect(sheet).toContain("from '$lib/title-case.svelte.ts'");
		expect(sheet).toContain('id="title-case"');
		expect(read('src/tools/manager/Panel.svelte')).toContain("from '$lib/title-case.svelte.ts'");
		// The words are cased where the fields are made, and nowhere else: one
		// import of the transform in the whole popup.
		expect(links).toContain("import { titleCase } from '$lib/title-case.ts'");
		expect(sheet).not.toContain("from '$lib/title-case.ts'");
	});

	it('are reached from the header of every view, as a sheet of their own', () => {
		for (const file of [
			'src/lib/components/ToolMenu.svelte',
			'src/lib/components/PanelShell.svelte',
			'src/lib/components/PanelGhost.svelte'
		]) {
			expect(read(file), file).toContain('SettingsButton');
		}

		expect(read('src/App.svelte')).toContain('loadSettingsSheet');
		expect(read('src/lib/settings-sheet.ts')).toContain("import('./components/SettingsSheet.svelte')");
		// Not a tool: it has no folder under src/tools and no entry in the registry.
		expect(read('src/lib/tools.ts')).not.toContain("id: 'settings'");
		expect(exists('src/tools/settings')).toBe(false);
	});
});

describe('stylesheets', () => {
	it('style every marker class the content script paints', () => {
		const css = read('content.css');

		for (const selector of [
			'.tbx-mark',
			'.tbx-mark--downloaded',
			'.tbx-mark--nodl',
			'.tbx-mark--small',
			'.tbx-mark__pill'
		]) {
			expect(css, selector).toContain(selector);
		}

		expect(css).toMatch(/pointer-events:\s*none/);
	});

	// Our elements sit inside SoundCloud's, so the page can reach them without
	// naming a class of ours — `.sound__coverArt .sc-artwork > button` is (0,2,1)
	// and beats any single class. The queue button carries its *state* in CSS
	// (a queued track's button is held up with no pointer on it, and that is the
	// only thing on the artwork saying the track is queued), so a rule of ours
	// that loses there does not merely look wrong, it hides the state. Every
	// selector that reaches the button therefore names it at least twice.
	it('weights every queue-button rule above a rule that names no class of ours', () => {
		const css = read('content.css');

		const selectors = [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{/g)]
			.flatMap((m) => m[1].split(','))
			.flatMap((one) => (one.includes('.tbx-queue') ? [one.trim()] : []));

		expect(selectors.length).toBeGreaterThan(0);

		for (const selector of selectors) {
			expect([...selector.matchAll(/\.tbx-queue/g)].length, selector).toBeGreaterThanOrEqual(2);
		}

		// And the one that holds a queued track's button up is there at all.
		expect(css).toContain(".tbx-queue.tbx-queue[data-state$=':yes']");
	});

	it('style the button that stands in for SoundCloud’s own, and the hiding of it', () => {
		const css = read('content.css');

		for (const selector of [
			'.tbx-dl',
			'.tbx-dl--soundcloud',
			'.tbx-dl--external',
			'.tbx-dl__icon',
			'.tbx-dl__text',
			'.tbx-dl-replaced'
		]) {
			expect(css, selector).toContain(selector);
		}

		// The one declaration that has to beat SoundCloud's own styling of
		// SoundCloud's own element, and the only `!important` in the file.
		const declarations = css.replace(/\/\*[\s\S]*?\*\//g, '');
		expect([...declarations.matchAll(/!important/g)]).toHaveLength(1);
		expect(css).toMatch(/\.tbx-dl-replaced \{\s*display: none !important;/);
	});

	// shadcn-svelte's components style their states with `data-checked:` and
	// friends; the bits-ui they run against says `data-state="checked"`. app.css
	// bridges the two, and both halves have to stay: a shim for a variant
	// nothing uses is dead weight, and a variant with no shim silently does
	// nothing at all.
	it('declares a variant for every state attribute the components style', () => {
		const ui = readdirSync(resolve(ROOT, 'src/lib/components/ui'), { recursive: true, encoding: 'utf8' })
			.filter((file) => file.endsWith('.svelte'))
			.map((file) => read(`src/lib/components/ui/${file}`))
			.join('\n');

		const used = new Set(
			[...ui.matchAll(/data-(checked|unchecked|open|closed|vertical|horizontal):/g)].map((m) => m[1])
		);

		expect(used.size).toBeGreaterThan(0);

		for (const name of used) {
			expect(appCss, `@custom-variant data-${name}`).toContain(`@custom-variant data-${name} `);
		}

		for (const [, name] of appCss.matchAll(/@custom-variant data-([a-z]+) /g)) {
			expect(used.has(name), `data-${name}: is declared but unused`).toBe(true);
		}
	});

	it('drives dark mode from a class, which the theme store sets', () => {
		expect(appCss).toContain('@custom-variant dark (&:is(.dark *));');
		expect(read('src/lib/theme.svelte.ts')).toContain("classList.toggle('dark'");
	});
});

describe('the exported state file', () => {
	const schemaPath = 'docs/manager-state.schema.json';
	const schema = JSON.parse(read(schemaPath));
	const transfer = read('state-transfer.ts');

	it('has a schema that the code and the docs both point at', () => {
		expect(exists(schemaPath)).toBe(true);

		for (const file of ['state-transfer.ts', 'src/lib/components/StateTransfer.svelte', 'README.md']) {
			expect(read(file), file).toContain('manager-state.schema.json');
		}
	});

	it('is validated by a script the package exposes', () => {
		expect(exists('scripts/validate-state.mts')).toBe(true);
		expect(JSON.parse(read('package.json')).scripts['validate:state']).toBe('node scripts/validate-state.mts');
	});

	// state-transfer.js writes the details cache that content.js owns; a rename
	// on either side would quietly stop restoring the looked-up details.
	it('is written under the same storage keys the rest of the extension uses', () => {
		// Each key is matched with or without a type annotation: the value is what has to agree.
		expect(transfer).toMatch(/const DETAILS_KEY(?:: [^=\n]+)? = 'details'(?: as const)?;/);
		expect(read('content.ts')).toContain("const DETAILS_KEY = 'details';");
		expect(transfer).toContain('TbxDownloaded.KEY');
		// The Manager's own list is named here for the same reason the details
		// cache is — one word, on both sides of a file that has to restore it.
		expect(transfer).toMatch(/const LISTED_KEY(?:: [^=\n]+)? = 'listed'(?: as const)?;/);
		expect(read('src/tools/manager/listed.svelte.ts')).toContain("export const LISTED_KEY = 'listed';");
		// And the artists tool's list, for the same reason again.
		expect(transfer).toMatch(/const ARTISTS_KEY(?:: [^=\n]+)? = 'artists'(?: as const)?;/);
		expect(read('src/tools/artists/artists.svelte.ts')).toContain("export const ARTISTS_KEY = 'artists';");
		// The queue is the one part it writes under no key of its own: the store
		// that owns that key is what an export reads from and an import writes
		// through (see "the session queue" above).
		expect(transfer).not.toContain('QUEUE_KEY');
		expect(read('src/lib/queue.svelte.ts')).toContain("const KEY = 'queue';");
	});

	// Everything the file carries has to be a thing the extension really keeps,
	// and every such thing has to be in the file — that is the whole promise of
	// "export the state". The queue was the one part left out of it.
	it('carries every member the schema declares, and nothing else', () => {
		expect(Object.keys(schema.properties)).toEqual([
			'format',
			'version',
			'exportedAt',
			'downloaded',
			'details',
			'listed',
			'artists',
			'queue',
			'prefs'
		]);

		for (const member of ['downloaded', 'details', 'listed', 'artists', 'queue', 'prefs']) {
			expect(transfer, member).toContain(`${member}:`);
		}
	});

	// The names in the schema's `prefs` are the localStorage keys the panels
	// actually read, so an option cannot be exported under a name nothing sets.
	it('carries the options the panels really keep', () => {
		expect(Object.keys(schema.properties.prefs.properties).sort()).toEqual(
			['background', 'hideDownloaded', 'sort'].sort()
		);
		const panel = read('src/tools/manager/Panel.svelte');

		for (const key of ['pref:background', 'pref:sort']) expect(panel).toContain(key);
		expect(read('src/lib/filter.ts')).toContain('pref:hide-downloaded');

		for (const key of ['pref:background', 'pref:sort', 'pref:hide-downloaded']) {
			expect(transfer, key).toContain(key);
		}
	});

	// In the settings sheet, not in a tool: the file carries the marks a
	// SoundCloud page paints, the content script's cache, the session queue and
	// three panels' options, and only one of its members is the Manager's.
	it('is offered and read back by the settings sheet', () => {
		const section = read('src/lib/components/StateTransfer.svelte');

		for (const call of ['snapshot(', 'serialize(', 'filename()', 'parse(', 'apply(']) {
			expect(section, call).toContain(call);
		}

		// And the queue and the list travel through their own stores in both
		// directions, which is what keeps the file honest about what the popup is
		// showing and keeps an import from holding a track twice.
		expect(section).toContain('snapshot(queue.entries, listed.entries, artists.entries)');
		expect(section).toContain('queue.add(doc.queue)');
		expect(section).toContain('listed.add(doc.listed)');
		expect(section).toContain('artists.add(doc.artists)');
		// The sheet is what the settings sheet draws, and the Manager's footer is
		// one press from it rather than a second copy of it.
		expect(read('src/lib/components/SettingsSheet.svelte')).toContain('<StateTransfer />');
		const panel = read('src/tools/manager/Panel.svelte');
		expect(panel).not.toContain('TbxState');
		expect(panel).toContain('view.settingsOpen = true');
	});

	// The promise the section makes, held down where it is made: adding is what
	// happens unless `replace` is asked for, and every store that an import
	// writes through offers both.
	it('adds by default, and replaces only when asked', () => {
		// Matched with or without type annotations: the defaults and the names are the promise.
		expect(transfer).toMatch(/\{ replace = false \}(?:: [^=\n]+)? = \{\}/);
		const section = read('src/lib/components/StateTransfer.svelte');
		expect(section).toContain('let replacing = $state(false);');
		expect(section).toContain('apply(doc, { replace })');

		for (const [file, call] of [
			['src/lib/queue.svelte.ts', /async replace\(tracks(?:: [^)\n]+)?\)/],
			['src/tools/manager/listed.svelte.ts', /async replace\(tracks(?:: [^)\n]+)?\)/],
			['src/tools/artists/artists.svelte.ts', /async replace\(artists(?:: [^)\n]+)?\)/],
			['downloaded-store.ts', /const replaceAll = async \(marks(?:: [^)\n]+)?\)/]
		] as const) {
			expect(read(file), file).toMatch(call);
		}
	});
});
