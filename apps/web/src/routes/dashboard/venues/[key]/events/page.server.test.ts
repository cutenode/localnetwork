import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { EventInput } from '#lib/server/services/events.ts';
import type { VenueView } from '#lib/server/services/venues.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const BOOKER = 'did:plc:booker';

let t: Awaited<ReturnType<typeof createTestServices>>;

let venue: VenueView;

let party: string;

let event: string;

const night = (overrides: Partial<EventInput> = {}): EventInput => ({
	name: 'Winter Opening',
	date: '2026-12-05',
	start: '23:00',
	venue: venue.space,
	audience: 'public',
	publish: true,
	...overrides
});

beforeEach(async () => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	const geo = { latitude: '52.51', longitude: '13.42' };
	venue = await t.venues.add(admin, { name: 'Tresor', address: { country: 'DE', locality: 'Berlin' }, geo });
	await t.sharedSpaces.addPerson(admin, venue.space, { did: BOOKER, role: 'booker' });
	party = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift' },
		avatar: 'keep',
		publish: true
	});
	event = await t.events.create(await t.viewer(HOST), party, night());
});

afterEach(async () => {
	vi.useRealTimers();
	await t.audit.idle();
	await t.db.destroy();
});

const on: Partial<Flags> = { peopleSpaces: true };

const opened = async (did: string, key = venue.skey, flags = on) =>
	loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(did),
		url: `/dashboard/venues/${key}/events`,
		params: { key },
		flags
	});

const approve = async (did: string, uri: string) =>
	callRoute(actions.approve, {
		app: t,
		viewer: await t.viewer(did),
		url: `/dashboard/venues/${venue.skey}/events?/approve`,
		params: { key: venue.skey },
		flags: on,
		form: { event: uri }
	});

it("lists the venue's events waiting for the venue, and confirms one", async () => {
	expect(await opened(BOOKER)).toEqual({
		venue: { name: 'Tresor' },
		waiting: [
			{ uri: event, name: 'Winter Opening', startsAt: '2026-12-05T23:00:00+01:00', party: 'Night Shift', lineup: 0 }
		]
	});

	expect(await approve(BOOKER, event)).toEqual({ success: 'Confirmed Winter Opening.' });
	expect((await opened(BOOKER)).waiting).toEqual([]);
	expect((await t.events.get(event))?.standing).toEqual({ listed: true, confirmed: true });
});

it('keeps out everyone else, and only approves published events naming this venue', async () => {
	await expect(opened(BOOKER, venue.skey, {})).rejects.toMatchObject({ status: 404 });
	await expect(opened(BOOKER, '3nothere')).rejects.toMatchObject({ status: 404 });
	await expect(opened(HOST)).rejects.toMatchObject({ status: 403 });
	expect((await opened(ADMIN)).waiting).toHaveLength(1);

	await expect(approve(BOOKER, `${event}x`)).rejects.toMatchObject({ status: 404 });

	const elsewhere = await t.venues.add(await t.viewer(ADMIN), {
		name: 'Berghain',
		address: { country: 'DE', locality: 'Berlin' },
		geo: { latitude: '52.51', longitude: '13.44' }
	});

	const host = await t.viewer(HOST);
	const other = await t.events.create(host, party, night({ name: 'Elsewhere', venue: elsewhere.space }));
	await expect(approve(BOOKER, other)).rejects.toMatchObject({ status: 404 });

	const draft = await t.events.create(host, party, night({ name: 'Still a draft', publish: false }));
	expect((await opened(BOOKER)).waiting.map((waiting) => waiting.name)).toEqual(['Winter Opening']);
	expect(await approve(BOOKER, draft)).toMatchObject({ status: 404, data: { error: 'That event does not exist' } });
});
