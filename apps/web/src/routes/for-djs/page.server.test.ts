import { afterEach, beforeEach, expect, it } from 'vitest';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { ProjectService } from '#lib/server/services/projects.ts';
import { ADMIN, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

const app = (projects: ProjectService = f.t.projects) => ({
	...f.t,
	projects,
	config: { publicUrl: 'https://localnetwork.test' }
});

it("describes the page for search engines and link previews at the site's own address", async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/for-djs' });

	expect(data).toMatchObject({
		meta: {
			title: 'For DJs',
			description: 'Get listed, or claim the listing an admin made, so promoters near you can find and book you.',
			url: 'https://localnetwork.test/for-djs'
		}
	});
});

it('shows a listing with enough filled in, and the directory totals', async () => {
	const admin = await f.t.viewer(ADMIN);
	// The filled-in listing first, so the newest one is bare and only the pick finds it.
	await f.t.projectAdmin.create({
		actor: admin,
		profile: {
			name: 'Filled In',
			kind: 'solo',
			tagline: 'Late-night jungle',
			genres: ['jungle'],
			fees: [{ tier: 'established', currency: 'GBP', minAmount: 300 }],
			location: { city: 'Leeds', country: 'GB' }
		}
	});
	await f.t.projectAdmin.create({
		actor: admin,
		profile: { name: 'Bare', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } }
	});

	const directory = await (await loadRoute<PageServerData>(load, { app: app(), url: '/for-djs' })).directory;

	expect(directory?.example?.profile.name).toBe('Filled In');
	expect(directory).toMatchObject({ djs: 3, cities: 2, unclaimed: 2 });
});

it('falls back to the newest listing when none is filled in', async () => {
	const directory = await (await loadRoute<PageServerData>(load, { app: app(), url: '/for-djs' })).directory;

	expect(directory?.example?.slug).toBe(f.project.slug);
});

it('still opens while HappyView is down', async () => {
	const offline = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const data = await loadRoute<PageServerData>(load, {
		app: app(new ProjectService(f.t.core, offline)),
		url: '/for-djs'
	});

	expect(await data.directory).toBeNull();
});

it("streams the next shows and the month's most played tracks, with two locallibrary captures", async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/for-djs' });

	expect(await data.shows).toEqual([]);
	expect(await data.tracks).toEqual([]);
	expect(data.captures.map((shown) => shown.line)).toEqual([
		'Keeps every copy of your library identical to the drive you export to, My Settings included',
		expect.stringMatching(/./)
	]);
	expect(
		data.captures.every(({ capture }) =>
			Object.keys(capture.picture.sources).every((format) => ['avif', 'webp'].includes(format))
		)
	).toBe(true);
});
