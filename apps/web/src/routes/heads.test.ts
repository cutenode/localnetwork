import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { expect, it } from 'vitest';
import { isUnlisted } from '#lib/server/http.ts';

const ROUTES = import.meta.dirname;

const SRC = join(ROUTES, '..');

const svelteFiles = (dir: string) =>
	readdirSync(dir, { recursive: true, withFileTypes: true })
		.filter((entry) => entry.isFile() && entry.name.endsWith('.svelte'))
		.map((entry) => join(entry.parentPath, entry.name));

/**
 * The public pages, as their folders under src/routes. The ones search engines skip (see isUnlisted) keep their own
 * head, since nothing previews them.
 */
const publicPages = svelteFiles(ROUTES)
	.filter((file) => file.endsWith('/+page.svelte'))
	.map((file) => relative(ROUTES, dirname(file)))
	.filter((folder) => !isUnlisted(`/${folder}`.replace(/\/$/, '') || '/'));

it("leaves public pages' heads to their loads' metadata, which the root layout writes", () => {
	const ownHeads = publicPages.filter((folder) =>
		readFileSync(join(ROUTES, folder, '+page.svelte'), 'utf8').includes('<svelte:head>')
	);

	expect(ownHeads).toEqual([]);
});

it('gives every public page a server load that returns its metadata', () => {
	const withoutMeta = publicPages.filter((folder) => {
		const load = join(ROUTES, folder, '+page.server.ts');

		return !existsSync(load) || !/\bmeta\b/.test(readFileSync(load, 'utf8'));
	});

	expect(withoutMeta).toEqual([]);
});

it("never waits on streamed data in a head, which crawlers don't run", () => {
	const awaitingHeads = svelteFiles(SRC).filter((file) =>
		[...readFileSync(file, 'utf8').matchAll(/<svelte:head>([\s\S]*?)<\/svelte:head>/g)].some(([, head]) =>
			head.includes('{#await')
		)
	);

	expect(awaitingHeads.map((file) => relative(SRC, file))).toEqual([]);
});
