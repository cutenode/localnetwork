import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { parseSpaceRecordUri, parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import type { VenueView } from '#lib/server/services/venues.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import { loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const OWNER = 'did:plc:owner';

const BOOKER = 'did:plc:booker';

const STRANGER = 'did:plc:stranger';

let t: Awaited<ReturnType<typeof createTestServices>>;

let venue: VenueView;

let party: string;

const on: Partial<Flags> = { spacePages: true, peopleSpaces: true };

const app = () => ({ ...t, config: { publicUrl: 'https://localnetwork.test' } });

const opened = async (path: string, { did = null, flags = on }: { did?: string | null; flags?: Partial<Flags> } = {}) =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: did ? await t.viewer(did) : null,
		url: path,
		params: { key: path.split(/[/?]/)[2], name: path.split('?')[0].split('/')[3] },
		flags
	});

const create = async (name: string, date: string, audience: 'public' | 'private' = 'public') =>
	t.events.create(await t.viewer(HOST), party, {
		name,
		date,
		start: '23:00',
		venue: venue.space,
		audience,
		publish: true
	});

beforeEach(async () => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	venue = await t.venues.add(admin, {
		name: 'Tresor',
		address: { country: 'DE', locality: 'Berlin', street: 'Köpenicker Str. 70' },
		geo: { latitude: '52.51', longitude: '13.42' }
	});
	await t.sharedSpaces.addPerson(admin, venue.space, { did: OWNER, role: 'owner' });
	await t.sharedSpaces.addPerson(admin, venue.space, { did: BOOKER, role: 'booker' });
	party = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift' },
		avatar: 'keep',
		publish: true
	});
});

afterEach(async () => {
	vi.useRealTimers();
	await t.audit.idle();
	await t.db.destroy();
});

it('shows where a venue is and its upcoming public events, waiting for the venue until it confirms them', async () => {
	const winter = await create('Winter Opening', '2026-12-05');
	await create('Private Birthday', '2026-12-06', 'private');
	await create('Last Week', '2026-11-06');
	const newYear = await create('New Year', '2026-12-31');
	await t.events.approve(await t.viewer(OWNER), newYear);
	const path = `/venue/${venue.skey}/tresor`;
	const partyKey = parseSpaceUri(party)!.skey;

	const { headers, loadData } = routeEvent({
		app: app(),
		url: path,
		params: { key: venue.skey, name: 'tresor' },
		flags: on
	});

	const page = await loadData<PageServerData>(load);

	expect(page.venue).toEqual({
		name: 'Tresor',
		address: { country: 'DE', locality: 'Berlin', street: 'Köpenicker Str. 70' },
		geo: { latitude: '52.51', longitude: '13.42' }
	});
	expect(page.events).toEqual([
		expect.objectContaining({
			uri: winter,
			href: `/party/${partyKey}/${parseSpaceRecordUri(winter)!.rkey}/winter-opening`,
			startsAt: '2026-12-05T23:00:00+01:00',
			status: EVENT_STATUS.scheduled,
			party: { name: 'Night Shift', href: `/party/${partyKey}/night-shift` },
			venue: { name: 'Tresor', href: path, locality: 'Berlin' },
			confirmed: false
		}),
		expect.objectContaining({ uri: newYear, confirmed: true })
	]);
	expect(page.dashboard).toEqual([]);
	expect(page.meta).toMatchObject({
		title: 'Tresor',
		description: 'Tresor in Berlin, DE: upcoming events on Local Network.',
		url: `https://localnetwork.test${path}`
	});
	expect(headers['cache-control']).toBe('public, max-age=60');
});

it('lists only Confirmed events under Venue approves', async () => {
	await t.venues.setApproval(await t.viewer(OWNER), venue.space, 'approval');
	await create('Waiting', '2026-12-05');
	const confirmed = await create('Confirmed', '2026-12-06');
	await t.events.approve(await t.viewer(OWNER), confirmed);

	expect((await opened(`/venue/${venue.skey}/tresor`)).events.map((event) => event.uri)).toEqual([confirmed]);
});

it('links its dashboard pages for the people who can use them, and claiming it for anyone else signed in', async () => {
	const path = `/venue/${venue.skey}/tresor`;
	const base = `/dashboard/venues/${venue.skey}`;
	const events = { label: 'Events to confirm', href: `${base}/events` };
	const settings = { label: 'Settings', href: `${base}/settings` };

	expect((await opened(path, { did: OWNER })).dashboard).toEqual([events, settings]);
	expect((await opened(path, { did: BOOKER })).dashboard).toEqual([events]);
	expect((await opened(path, { did: ADMIN })).dashboard).toEqual([events, settings]);
	expect((await opened(path, { did: STRANGER })).dashboard).toEqual([
		{ label: 'Claim this venue', href: `${base}/claim` }
	]);
	expect((await opened(path, { did: OWNER, flags: { spacePages: true } })).dashboard).toEqual([]);
});

it('keeps no copy for its people and staff, who can change what it shows, and lets the browser keep one for anyone else', async () => {
	const cacheControl = async (did: string, flags = on) => {
		const { headers, loadData } = routeEvent({
			app: app(),
			viewer: await t.viewer(did),
			url: `/venue/${venue.skey}/tresor`,
			params: { key: venue.skey, name: 'tresor' },
			flags
		});

		await loadData<PageServerData>(load);

		return headers['cache-control'];
	};

	expect(await cacheControl(OWNER)).toBe('no-store');
	expect(await cacheControl(BOOKER)).toBe('no-store');
	expect(await cacheControl(ADMIN)).toBe('no-store');
	// Its people change it from their dashboard, which they reach without peopleSpaces too.
	expect(await cacheControl(OWNER, { spacePages: true })).toBe('no-store');
	expect(await cacheControl(STRANGER)).toBe('private, max-age=60');
});

it('sends any other name, or none, on to the current one, and is a 404 for an unknown venue or while spacePages is off', async () => {
	await expect(opened(`/venue/${venue.skey}?ref=x`)).rejects.toMatchObject({
		status: 308,
		location: `/venue/${venue.skey}/tresor?ref=x`
	});
	await expect(opened(`/venue/${venue.skey}/old-name`)).rejects.toMatchObject({
		status: 308,
		location: `/venue/${venue.skey}/tresor`
	});
	await expect(opened('/venue/3nothing/tresor')).rejects.toMatchObject({ status: 404 });
	await expect(opened(`/venue/${venue.skey}/tresor`, { flags: {} })).rejects.toMatchObject({ status: 404 });
});

it('describes a venue it only knows the country of, without a map', async () => {
	const somewhere = await t.venues.add(await t.viewer(ADMIN), { name: 'Somewhere', address: { country: 'DE' } });
	const page = await opened(`/venue/${somewhere.skey}/somewhere`);

	expect(page.venue.geo).toBeNull();
	expect(page.meta.description).toBe('Somewhere in DE: upcoming events on Local Network.');
});
