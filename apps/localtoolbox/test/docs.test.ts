// @vitest-environment node
//
// The README against the code it describes.
//
// This README is not a summary; it is the specification — how each gate site
// works, what every option does, which permissions are asked for and why. That
// makes it the one part of the repository that can be wrong without anything
// failing, and the part most likely to be wrong, because it is the part a
// change does not force anyone to open.
//
// So: every tool, option, format, size and site the code offers has to be
// named in it, every file it points at has to be there, every command it
// tells someone to run has to exist, and every link inside it has to lead
// somewhere.
import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { SITES } from '../src/tools/third-party-downloads/sites/index.ts';

const ROOT = resolve(import.meta.dirname, '..');

const read = (path: string) => readFileSync(resolve(ROOT, path), 'utf8');

const README = read('README.md');

const pkg = JSON.parse(read('package.json'));

const manifest = JSON.parse(read('src/manifest.json'));

// Code fences, so prose can be checked without matching the samples inside it.
const FENCES = [...README.matchAll(/```(\w*)\n([\s\S]*?)```/g)].map((m) => ({
	language: m[1],
	body: m[2]
}));

const PROSE = README.replace(/```[\s\S]*?```/g, '');

const headings = [...README.matchAll(/^(#{1,6}) (.+)$/gm)].map((m) => ({
	level: m[1].length,
	text: m[2].trim()
}));

// GitHub's anchor for a heading: lower-cased, punctuation dropped, spaces to
// dashes.
const anchor = (text: string) =>
	text
		.toLowerCase()
		.replace(/[^\w\s-]/g, '')
		.trim()
		.replace(/\s+/g, '-');

describe('what the README claims exists', () => {
	it('names every tool the popup offers', () => {
		const registry = read('src/lib/tools.ts');
		const names = [...registry.matchAll(/name: '([^']+)'/g)].map((m) => m[1]);
		expect(names.length).toBe(4);

		for (const name of names) expect(PROSE, name).toContain(name);

		// And gives each one a section of its own.
		for (const name of names) {
			expect(
				headings.some((heading) => heading.text.includes(name)),
				`no heading for ${name}`
			).toBe(true);
		}
	});

	it('names every output format and search site', () => {
		const labels = (file: string, field = 'label') =>
			[...read(file).matchAll(new RegExp(`${field}: '([^']+)'`, 'g'))].map((m) => m[1]);

		for (const label of labels('src/tools/soundcloud-playlist-to-text/formats.ts')) {
			expect(PROSE, label).toContain(label);
		}

		// The search badges are a file of their own, so the table in the README
		// is checked against it rather than against any code: a site added there
		// is one nobody has to remember to write down, because this fails until
		// it is.
		for (const site of JSON.parse(read('src/lib/search-sites.json'))) {
			expect(PROSE, site.name).toContain(site.name);
			expect(README, site.url).toContain(site.url);
			// A cell of its own; the formatter pads a table's columns to one width.
			expect(README, site.badge).toMatch(new RegExp(`\\| ${RegExp.escape(site.badge)} +\\|`));
		}
	});

	// Every variable a search URL may carry, since the file's format is the one
	// thing here somebody outside this repository is invited to write.
	it('names every variable a search template can use', () => {
		// Its type annotation is a function type, so an `=` before the one that assigns is an arrow's.
		const block = /const VARIABLES(?:: (?:[^=\n]|=>)+)? = \{([\s\S]*?)^\}(?: satisfies [^;]+)?;/m.exec(
			read('src/lib/search-sites.ts')
		)![1];

		const names = [...block.matchAll(/^\t(\w+):/gm)].map((m) => m[1]);

		expect(names.length).toBe(3);

		for (const name of names) expect(README, name).toContain(`{${name}}`);
	});

	// The registry rather than a list of its files: a site added there is one
	// this fails on until it is written down, and one module can hold two
	// sites (toneden.js is ToneDen and FanLink).
	it('names every gate site the Downloader walks, and its hosts', () => {
		expect(SITES.length).toBeGreaterThan(5);

		for (const site of SITES) {
			expect(PROSE, `${site.id}: ${site.name}`).toContain(site.name);

			for (const host of [...site.hosts, ...(site.aliases ?? [])]) {
				expect(README, `${site.id}: ${host}`).toContain(host);
			}
		}
	});

	// The settings sheet is the one place the extension itself can be changed
	// from, and a switch nobody wrote down is a switch nobody knows they have.
	// Each is looked for by the words on it, which is what somebody reading the
	// README would go looking for.
	it('names every setting the sheet offers, by the label on it', () => {
		const sheet = read('src/lib/components/SettingsSheet.svelte');
		// Every label in the sheet, not only the page's settings: the Downloader's
		// address and marking, and title casing, are switches somebody reading
		// the README goes looking for just the same.
		const labels = [...sheet.matchAll(/<Label for="([^"]+)"[^>]*>\s*([^<]+?)\s*<\/Label>/g)];
		expect(labels.length).toBeGreaterThanOrEqual(6);

		// Each of the settings the content script obeys has one: the switch for a
		// setting is the one whose id is its name — the same words, as a page
		// writes them.
		const names = [
			.../export const DEFAULTS(?:: [^=\n]+)? = \{([\s\S]*?)\};/
				.exec(read('src/lib/settings.svelte.ts'))![1]
				.matchAll(/^\s+(\w+):/gm)
		].map((m) => m[1]);

		expect(names.length).toBeGreaterThan(1);

		for (const name of names) {
			const id = name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
			expect(sheet, id).toContain(`id="${id}"`);
			expect(
				labels.map((label) => label[1]),
				`no label for ${id}`
			).toContain(id);
		}

		// The sheet types an apostrophe the way a screen wants one and this file
		// types the one a keyboard has; they are the same word. And the README
		// wraps its lines where the sheet does not.
		const words = (s: string) => s.replace(/’/g, "'").replace(/\s+/g, ' ');
		const prose = words(PROSE);

		for (const [, id, label] of labels) expect(prose, `${id}: ${label}`).toContain(words(label));
	});

	it('lists exactly the permissions the manifest asks for', () => {
		for (const permission of manifest.permissions) {
			expect(README, permission).toContain(permission);
		}

		for (const pattern of manifest.host_permissions) {
			const host = /^https:\/\/(?:\*\.)?([^/]+)\/\*$/.exec(pattern)![1];
			expect(README, host).toContain(host);
		}
	});

	it('tells people to run commands the package really has', () => {
		const commands = new Set(
			FENCES.filter((fence) => fence.language === '' || fence.language === 'bash')
				.flatMap((fence) => [...fence.body.matchAll(/^pnpm ([\w:]+)/gm)])
				// `install` is pnpm's own command, run at the repo root, rather than a script.
				.flatMap(([, command]) => (command === 'install' ? [] : [command]))
		);

		expect(commands.size).toBeGreaterThan(2);

		for (const command of commands) expect(Object.keys(pkg.scripts), command).toContain(command);
	});

	it('points only at files that are there', () => {
		// Paths in backticks that look like real ones: a folder this repo has, or
		// a file with an extension it uses.
		const mentioned = new Set(
			[...README.matchAll(/`([\w./-]+\.(?:ts|mts|js|json|css|html|svelte|mjs|md))`/g)].map((m) => m[1])
		);

		expect(mentioned.size).toBeGreaterThan(10);
		const ROOTED = /^(src|test|docs|scripts|icons|dist)\//;

		for (const path of mentioned) {
			// Only paths written from the repository root: the rest are named
			// relative to whatever folder the section is discussing.
			if (!ROOTED.test(path)) continue;
			expect(existsSync(resolve(ROOT, path)), path).toBe(true);
		}
	});

	it('draws a tree of the test suite that matches the suite', () => {
		const tree = FENCES.find((fence) => fence.body.startsWith('test/'));
		expect(tree, 'no test/ tree in the README').toBeTruthy();
		// Every file it names, wherever on a line: some lines name two.
		const named = [...tree!.body.matchAll(/(?:test\/)?([\w./-]+\.test\.ts)/g)].map((m) => m[1]);

		for (const file of named) {
			expect(existsSync(resolve(ROOT, 'test', file)) || existsSync(resolve(ROOT, file)), file).toBe(true);
		}

		// And the other way round: every test file at the top of test/ is named,
		// and every folder of them is, by its folder.
		const suite = readdirSync(resolve(ROOT, 'test'), { withFileTypes: true });

		const files = suite.flatMap((entry) => (entry.isFile() && entry.name.endsWith('.test.ts') ? [entry.name] : []));

		const folders = suite.flatMap((entry) =>
			entry.isDirectory() && readdirSync(resolve(ROOT, 'test', entry.name)).some((file) => file.endsWith('.test.ts'))
				? [entry.name]
				: []
		);

		for (const file of files) expect(named, `${file} is not in the README's tree`).toContain(file);

		for (const folder of folders) {
			expect(tree!.body, `${folder}/ is not in the README's tree`).toMatch(new RegExp(`(^|\\s)${folder}/`));
		}
	});

	it('has no link that leads nowhere inside itself', () => {
		for (const [, , target] of README.matchAll(/\[([^\]]+)\]\((#[^)]+)\)/g)) {
			const wanted = target.slice(1);
			expect(
				headings.some((heading) => anchor(heading.text) === wanted),
				`no heading for ${target}`
			).toBe(true);
		}
	});

	it('has JSON samples that are JSON', () => {
		const json = FENCES.filter((fence) => fence.language === 'json');
		expect(json.length).toBeGreaterThan(0);

		for (const fence of json) expect(() => JSON.parse(fence.body)).not.toThrow();
	});
});

describe('what the package says about itself', () => {
	it('agrees with the manifest about the version', () => {
		expect(pkg.version).toBe(manifest.version);
	});

	it('offers a script for everything the README tells people to do', () => {
		for (const script of ['build', 'dev', 'test', 'test:coverage', 'verify', 'zip', 'validate:state']) {
			expect(Object.keys(pkg.scripts), script).toContain(script);
		}
	});

	it('ships no runtime dependency into the extension', () => {
		// Everything in `dependencies` is compiled into the popup bundle; nothing
		// is loaded at runtime, and the content script imports nothing at all.
		expect(read('content.ts')).not.toMatch(/^\s*import\s/m);
		expect(read('downloaded-store.ts')).not.toMatch(/^\s*import\s/m);
		expect(read('state-transfer.ts')).not.toMatch(/^\s*import\s/m);
	});
});
