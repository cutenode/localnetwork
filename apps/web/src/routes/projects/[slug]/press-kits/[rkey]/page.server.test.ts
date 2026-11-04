import { afterEach, beforeEach, expect, it } from 'vitest';
import { silentLogger } from '#lib/server/logger.ts';
import { DjService } from '#lib/server/services/dj.ts';
import { ROOT } from '#lib/server/testing/core.ts';
import { DJ, FRIEND, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { loadRoute, routeEvent, type RouteEventInit } from '#lib/testing/route-event.ts';
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

/** The press kit page's event for `rkey` and `did` (signed out without one), with the services the page reads. */
const kitPage = async (rkey: string, did: string | null): Promise<RouteEventInit> => ({
	app: {
		...f.t,
		dj: new DjService(f.t.indexedRepo, silentLogger),
		config: { publicUrl: 'https://localnetwork.test' }
	},
	viewer: did ? await f.t.viewer(did) : null,
	url: `/projects/${f.project.slug}/press-kits/${rkey}`,
	params: { slug: f.project.slug, rkey }
});

/** What the press kit page `rkey` loads for `did`. */
const pressKit = async (rkey: string, did: string | null) => loadRoute<PageServerData>(load, await kitPage(rkey, did));

/** The project with booking details on file, which only its managers and people let in see. */
async function withDetails() {
	const { project } = await f.t.projects.publish({
		existing: f.project,
		profile: f.project.profile,
		managers: f.project.managers,
		status: 'active',
		createdBy: ROOT,
		details: { bookingEmail: 'book@neontide.test' }
	});

	f.project = project;
}

it('shows someone signed out the public part, and what they could get with an account', async () => {
	const epk = await f.publish(DJ);
	const data = await pressKit(epk.rkey, null);

	expect(data).toMatchObject({
		epk: { rkey: epk.rkey },
		access: { kind: 'signedOut', rule: 'review' },
		content: null,
		details: null,
		tracklists: {},
		meta: { title: 'Neon Tide: Clubs' }
	});
});

it("writes the press kit's record tags and structured data into the head for crawlers", async () => {
	const epk = await f.publish(DJ);
	const page = routeEvent(await kitPage(epk.rkey, null));

	await page.call(load);

	expect(page.locals).toMatchObject({
		crawlerHead: expect.stringContaining(`<meta name="at:canonical" content="${epk.uri}" />`)
	});
	expect(page.locals).toMatchObject({ crawlerHead: expect.stringContaining('"@type":"ProfilePage"') });
});

it('shows the whole press kit to someone let in, with the booking details, and gives the managers a receipt', async () => {
	await withDetails();
	const epk = await f.publish(DJ);
	await f.t.epks.invite(await f.t.viewer(DJ), f.kit(epk), FRIEND);

	const data = await pressKit(epk.rkey, FRIEND);

	expect(data).toMatchObject({
		access: { kind: 'granted' },
		content: { bio: 'The long bio', techRider: '2x CDJ-3000' },
		details: { bookingEmail: 'book@neontide.test' }
	});
	expect((await f.t.epkViews.list(f.project)).map((view) => view.viewer.did)).toEqual([FRIEND]);
});

it('shows someone let in no booking details when the project has none', async () => {
	const epk = await f.publish(DJ);
	await f.t.epks.invite(await f.t.viewer(DJ), f.kit(epk), FRIEND);

	expect((await pressKit(epk.rkey, FRIEND)).details).toEqual({});
});

it('shows a manager the whole press kit without recording a view', async () => {
	await withDetails();
	const epk = await f.publish(DJ);

	const data = await pressKit(epk.rkey, DJ);

	expect(data).toMatchObject({
		access: { kind: 'manager' },
		content: { bio: 'The long bio' },
		details: { bookingEmail: 'book@neontide.test' }
	});
	expect(await f.t.epkViews.list(f.project)).toEqual([]);
});

it('keeps the rest from someone who only asked', async () => {
	const epk = await f.publish(DJ);
	await f.t.epks.request(await f.t.viewer(PROMOTER), f.kit(epk), {});

	expect(await pressKit(epk.rkey, PROMOTER)).toMatchObject({
		access: { kind: 'pending' },
		content: null,
		details: null
	});
});

it("numbers the project's mixes that have tracklists, by their address", async () => {
	const tracklist = await f.t.net.client(ROOT).uploadBlob(new TextEncoder().encode('{}'), 'application/json');

	const { project } = await f.t.projects.publish({
		existing: f.project,
		profile: {
			...f.project.profile,
			mixes: [
				{ title: 'Boiler Room', url: 'https://soundcloud.com/neon/boiler-room', tracklist },
				{ title: 'Radio', url: 'https://soundcloud.com/neon/radio' },
				{ title: 'Closing set', url: 'https://soundcloud.com/neon/closing', tracklist }
			]
		},
		managers: f.project.managers,
		status: 'active',
		createdBy: ROOT
	});

	f.project = project;
	const epk = await f.publish(DJ);

	const data = await pressKit(epk.rkey, null);

	expect(data.tracklists).toEqual({
		'https://soundcloud.com/neon/boiler-room': 1,
		'https://soundcloud.com/neon/closing': 3
	});
});

it("answers a press kit that isn't there with a 404", async () => {
	await expect(pressKit('3lbnxyk5m2s2a', null)).rejects.toMatchObject({
		status: 404,
		body: { message: 'Press kit not found' }
	});
});
