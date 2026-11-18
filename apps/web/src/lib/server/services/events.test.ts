import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { ForbiddenError, NotFoundError, UserError } from '../core';
import { performerUri } from '../performer-uri';
import { createTestServices, ROOT } from '../testing/core';
import { type EventInput, ZoneChangeError } from './events';
import { STALE_PAGE } from './shared-spaces';
import type { VenueView } from './venues';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const DOOR = 'did:plc:door';

const OWNER = 'did:plc:owner';

const BOOKER = 'did:plc:booker';

const DJ = performerUri(ROOT, { kind: 'dj', did: 'did:plc:dj' });

describe('EventService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let host: Viewer;
	let party: string;
	let venue: VenueView;
	let act: string;

	beforeEach(async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
		t = await createTestServices({ admins: [ADMIN] });
		const admin = await t.viewer(ADMIN);
		host = await t.viewer(HOST);
		party = await t.parties.start(host, { basics: { name: 'Night Shift' }, avatar: 'keep', publish: true });
		await t.sharedSpaces.addPerson(admin, party, { did: DOOR, role: 'door' });
		venue = await t.venues.add(admin, {
			name: 'Tresor',
			address: { country: 'DE', locality: 'Berlin' },
			geo: { latitude: '52.51', longitude: '13.42' }
		});
		await t.sharedSpaces.addPerson(admin, venue.space, { did: OWNER, role: 'owner' });
		await t.sharedSpaces.addPerson(admin, venue.space, { did: BOOKER, role: 'booker' });
		const actSpace = await t.sharedSpaces.createAct(host, { slug: 'crew', form: 'duo', basics: { name: 'Crew' } });
		act = (await t.performerRegistry.byKey(actSpace.split('/').at(-1)!))!.uri;
	});
	afterEach(async () => {
		vi.useRealTimers();
		await t.audit.idle();
		await t.db.destroy();
	});

	const input = (overrides: Partial<EventInput> = {}): EventInput => ({
		name: 'Winter Opening',
		date: '2026-12-05',
		start: '23:00',
		end: '06:00',
		venue: venue.space,
		audience: 'public',
		publish: true,
		...overrides
	});

	/** Only a name: what a new draft starts with. */
	const bare: EventInput = { name: 'Winter Opening', audience: 'public', publish: false };

	const stored = (uri: string) => {
		const rkey = uri.split('/').at(-1)!;

		return t.net.spaceRecords.get(`${party}|${ROOT}|${COLLECTION.event}|${rkey}`)!;
	};

	const audited = async (action: string) => {
		await t.audit.idle();

		return (await t.audit.list({ action })).map(({ actorRole, summary, changes }) => ({
			actorRole,
			summary,
			changes: JSON.parse(changes ?? '[]')
		}));
	};

	it('creates a published event in the party space, on the venue clock, for someone with events.manage', async () => {
		const uri = await t.events.create(host, party, {
			...input({ tickets: 'https://tickets.example/winter', description: ' Doors at eleven. ' }),
			lineup: [{ performer: act, start: '23:00', end: '02:00' }, { performer: DJ, start: '02:00' }, { performer: DJ }]
		});

		const record = stored(uri).value;
		expect(() => validateRecord(COLLECTION.event, record)).not.toThrow();
		expect(record).toMatchObject({
			name: 'Winter Opening',
			description: 'Doors at eleven.',
			startsAt: '2026-12-05T23:00:00+01:00',
			endsAt: '2026-12-06T06:00:00+01:00',
			venue: venue.space,
			tickets: 'https://tickets.example/winter',
			status: EVENT_STATUS.scheduled,
			audience: 'public',
			publishedAt: '2026-11-13T12:00:00.000Z',
			lineup: [
				{ performer: act, startsAt: '2026-12-05T23:00:00+01:00', endsAt: '2026-12-06T02:00:00+01:00' },
				{ performer: DJ, startsAt: '2026-12-06T02:00:00+01:00' },
				{ performer: DJ }
			],
			createdBy: HOST
		});
		expect(record.linkToken).toBeUndefined();
		expect(await t.events.ofParty(party)).toEqual([
			expect.objectContaining({ uri, space: party, cid: stored(uri).cid })
		]);
		expect(await audited('event.create')).toEqual([
			{ actorRole: 'member', summary: 'Created Winter Opening', changes: [] }
		]);

		await expect(t.events.create(await t.viewer(DOOR), party, input())).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.events.create(host, venue.space, input())).rejects.toBeInstanceOf(NotFoundError);
	});

	it('refuses an event without a name, at a venue it cannot place, with a bad ticket link or a lineup that is gone', async () => {
		const placeless = await t.venues.add(host, { name: 'Nowhere', address: { country: 'DE' } });

		await expect(t.events.create(host, party, input({ name: ' ' }))).rejects.toThrow(
			new UserError('Give the event a name')
		);
		await expect(t.events.create(host, party, input({ venue: `${venue.space}x` }))).rejects.toThrow(
			new UserError('Pick a venue')
		);
		await expect(t.events.create(host, party, input({ venue: placeless.space }))).rejects.toThrow(
			new UserError("That venue has no location yet, so its clock isn't known")
		);
		await expect(t.events.create(host, party, input({ tickets: 'tickets.example' }))).rejects.toThrow(
			new UserError('Use a full web address for tickets, starting with https://')
		);
		await expect(
			t.events.create(host, party, input({ lineup: [{ performer: `at://${ROOT}/${COLLECTION.listing}/3gone` }] }))
		).rejects.toThrow(new UserError('Someone on the lineup is no longer on the site'));
	});

	describe('drafts', () => {
		it('keeps a draft with only a name, out of every list and closed to visitors', async () => {
			const uri = await t.events.create(host, party, bare);

			expect(stored(uri).value).toEqual({
				$type: COLLECTION.event,
				name: 'Winter Opening',
				status: EVENT_STATUS.scheduled,
				audience: 'public',
				timeZone: 'UTC',
				createdBy: HOST,
				createdAt: '2026-11-13T12:00:00.000Z',
				updatedAt: '2026-11-13T12:00:00.000Z'
			});
			expect(await t.events.get(uri)).toMatchObject({ venue: null, approved: false, standing: { listed: false } });
			expect(await t.events.open(uri, null)).toBeNull();
			expect(await audited('event.create')).toEqual([
				{ actorRole: 'member', summary: 'Created Winter Opening as a draft', changes: [] }
			]);

			// Named at a venue, a draft still isn't listed there or waiting for it, and the venue can't approve it.
			const named = await t.events.create(host, party, input({ publish: false }));
			expect(await t.events.get(named)).toMatchObject({ standing: { listed: false, confirmed: false } });
			expect(await t.events.waiting(venue.space)).toEqual([]);
			await expect(t.events.approve(await t.viewer(OWNER), named)).rejects.toBeInstanceOf(NotFoundError);
			// Undated drafts come after dated events.
			expect((await t.events.ofParty(party)).map((event) => event.uri)).toEqual([named, uri]);

			// Saving a draft without publishing keeps it a draft.
			await t.events.update(host, uri, { ...bare, name: 'Winter Opening II', swap: stored(uri).cid });
			expect(stored(uri).value).toMatchObject({ name: 'Winter Opening II' });
			expect(stored(uri).value.publishedAt).toBeUndefined();
		});

		it('needs a date, a venue and a published party to publish, and times need a date and a start', async () => {
			for (const missing of [{ date: undefined, start: undefined }, { venue: undefined }])
				await expect(t.events.create(host, party, input({ ...missing, end: undefined }))).rejects.toThrow(
					new UserError('Add a date and a venue to publish it')
				);

			await expect(t.events.create(host, party, { ...bare, date: '2026-12-05' })).rejects.toThrow(
				new UserError('Add the date and when it starts')
			);
			await expect(t.events.create(host, party, { ...bare, start: '23:00' })).rejects.toThrow(
				new UserError('Add the date and when it starts')
			);
			await expect(
				t.events.create(host, party, { ...bare, venue: venue.space, lineup: [{ performer: DJ, start: '01:00' }] })
			).rejects.toThrow(new UserError("Add the night's date before set times"));

			// A venue without times is fine for a draft.
			const uri = await t.events.create(host, party, { ...bare, venue: venue.space });
			expect(stored(uri).value).toMatchObject({ venue: venue.space });
			expect(stored(uri).value.startsAt).toBeUndefined();

			// A published event never shows a party that's still a draft.
			const draftParty = await t.parties.start(host, {
				basics: { name: 'Next Thing' },
				avatar: 'keep',
				publish: false
			});

			await expect(t.events.create(host, draftParty, input())).rejects.toThrow(
				new UserError('Publish the party first')
			);
			await t.events.create(host, draftParty, input({ publish: false }));
		});

		it('publishes a draft once, keeping when it was first published, without calling it rescheduled', async () => {
			const uri = await t.events.create(host, party, input({ publish: false }));

			vi.setSystemTime(new Date('2026-11-14T09:00:00Z'));
			await t.events.update(host, uri, { ...input({ start: '22:00' }), swap: stored(uri).cid });
			expect(stored(uri).value).toMatchObject({
				publishedAt: '2026-11-14T09:00:00.000Z',
				status: EVENT_STATUS.scheduled
			});
			expect(await t.events.open(uri, null)).toMatchObject({ uri, standing: { listed: true, confirmed: false } });

			await t.events.update(host, uri, { ...input({ start: '22:00', publish: false }), swap: stored(uri).cid });
			expect(stored(uri).value.publishedAt).toBe('2026-11-14T09:00:00.000Z');
			// A published event can't lose what publishing it needed.
			await expect(
				t.events.update(host, uri, { ...input({ venue: undefined, publish: false }), swap: stored(uri).cid })
			).rejects.toThrow(new UserError('Add a date and a venue to publish it'));

			// Newest first.
			expect((await audited('event.update')).map(({ summary }) => summary)).toEqual([
				'Updated Winter Opening',
				'Published Winter Opening'
			]);
		});
	});

	describe('time zones', () => {
		const BERLIN = { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

		const NEW_YORK = { city: 'New York', country: 'US', latE6: 40_712_800, lngE6: -74_006_000 };

		const night = { date: '2026-12-05', start: '23:00', end: '06:00' };

		/** A published party, with `location` as its area. */
		const partyIn = (location: { city: string; country: string; latE6?: number; lngE6?: number }) =>
			t.parties.start(host, { basics: { name: 'Elsewhere', location }, avatar: 'keep', publish: true });

		it("keeps a venue-less draft's times on its party's area, else its creator's, else UTC", async () => {
			const utc = await t.events.create(host, party, { ...bare, ...night });
			expect(stored(utc).value).toMatchObject({
				startsAt: '2026-12-05T23:00:00Z',
				endsAt: '2026-12-06T06:00:00Z',
				timeZone: 'UTC'
			});

			const berlin = await partyIn(BERLIN);
			const inBerlin = await t.events.create(host, berlin, { ...bare, ...night });
			expect(
				t.net.spaceRecords.get(`${berlin}|${ROOT}|${COLLECTION.event}|${inBerlin.split('/').at(-1)}`)?.value
			).toMatchObject({ startsAt: '2026-12-05T23:00:00+01:00', timeZone: 'Europe/Berlin' });

			// A city typed without coordinates gives no zone, so the creator's profile does.
			await t.net.client(HOST).putRecord({
				collection: COLLECTION.profile,
				rkey: 'self',
				record: {
					$type: COLLECTION.profile,
					basics: { name: 'Host', location: NEW_YORK },
					createdAt: '2026-10-30T00:00:00.000Z',
					updatedAt: '2026-10-30T00:00:00.000Z'
				}
			});
			const typed = await partyIn({ city: 'Berlin', country: 'DE' });
			const inNewYork = await t.events.create(host, typed, { ...bare, ...night });
			expect(
				t.net.spaceRecords.get(`${typed}|${ROOT}|${COLLECTION.event}|${inNewYork.split('/').at(-1)}`)?.value
			).toMatchObject({ startsAt: '2026-12-05T23:00:00-05:00', timeZone: 'America/New_York' });
		});

		it('takes a time zone the draft is given, keeps it through saves, and refuses one that is not real', async () => {
			const uri = await t.events.create(host, party, { ...bare, ...night, timeZone: 'Asia/Kolkata' });
			expect(stored(uri).value).toMatchObject({ startsAt: '2026-12-05T23:00:00+05:30', timeZone: 'Asia/Kolkata' });

			await t.events.update(host, uri, { ...bare, ...night, name: 'Renamed', swap: stored(uri).cid });
			expect(stored(uri).value).toMatchObject({ startsAt: '2026-12-05T23:00:00+05:30', timeZone: 'Asia/Kolkata' });

			await expect(t.events.create(host, party, { ...bare, timeZone: 'Mars/Olympus' })).rejects.toThrow(
				new UserError('Pick a time zone from the list')
			);
		});

		it("hands a draft's times to the venue's clock, asking first when that moves them", async () => {
			const lineup = [{ performer: DJ, start: '01:00', end: '03:00' }];
			const uri = await t.events.create(host, party, { ...bare, ...night, lineup });

			const save = (overrides: Partial<EventInput>) =>
				t.events.update(host, uri, { ...input({ publish: false, lineup, ...overrides }), swap: stored(uri).cid });

			// UTC's 23:00 is midnight in Berlin, so picking Tresor asks which to keep.
			await expect(save({})).rejects.toBeInstanceOf(ZoneChangeError);
			await expect(save({})).rejects.toMatchObject({
				from: 'UTC',
				to: 'Europe/Berlin',
				options: { clock: '2026-12-05T23:00:00+01:00', instant: '2026-12-06T00:00:00+01:00' }
			});

			await save({ zoneChange: 'instant' });
			expect(stored(uri).value).toMatchObject({
				startsAt: '2026-12-06T00:00:00+01:00',
				endsAt: '2026-12-06T07:00:00+01:00',
				venue: venue.space,
				lineup: [{ performer: DJ, startsAt: '2026-12-06T02:00:00+01:00', endsAt: '2026-12-06T04:00:00+01:00' }]
			});
			expect(stored(uri).value.timeZone).toBeUndefined();

			// Back to a draft on New York's clock, then on to the venue keeping the times as entered.
			await save({ venue: undefined, timeZone: 'America/New_York', zoneChange: 'clock' });
			expect(stored(uri).value).toMatchObject({ startsAt: '2026-12-05T23:00:00-05:00', timeZone: 'America/New_York' });
			await save({ zoneChange: 'clock' });
			expect(stored(uri).value).toMatchObject({ startsAt: '2026-12-05T23:00:00+01:00', venue: venue.space });
		});

		it('moves a published event to a venue on another clock without calling it rescheduled, kept at the same moments', async () => {
			const uri = await t.events.create(host, party, input());

			const london = await t.venues.add(await t.viewer(ADMIN), {
				name: 'Fabric',
				address: { country: 'GB', locality: 'London' },
				geo: { latitude: '51.52', longitude: '-0.10' }
			});

			await expect(
				t.events.update(host, uri, { ...input({ venue: london.space }), swap: stored(uri).cid })
			).rejects.toBeInstanceOf(ZoneChangeError);
			await t.events.update(host, uri, {
				...input({ venue: london.space, zoneChange: 'instant' }),
				swap: stored(uri).cid
			});
			expect(stored(uri).value).toMatchObject({
				startsAt: '2026-12-05T22:00:00Z',
				venue: london.space,
				status: EVENT_STATUS.scheduled
			});
		});

		it('asks nothing when the new clock reads the same moments', async () => {
			const uri = await t.events.create(host, party, { ...bare, ...night, end: undefined, timeZone: 'Europe/Paris' });

			await t.events.update(host, uri, { ...input({ publish: false, end: undefined }), swap: stored(uri).cid });
			expect(stored(uri).value).toMatchObject({ startsAt: '2026-12-05T23:00:00+01:00', venue: venue.space });
		});
	});

	describe('private events', () => {
		it('opens only with its link token, for anyone, and is never listed', async () => {
			const uri = await t.events.create(host, party, input({ audience: 'private' }));
			const token = (await t.events.get(uri))?.record.linkToken ?? '';

			expect(token).toMatch(/^[\w-]{43}$/);
			expect(await t.events.open(uri, null)).toBeNull();
			expect(await t.events.open(uri, 'x'.repeat(43))).toBeNull();
			expect(await t.events.open(uri, token.slice(1))).toBeNull();
			expect(await t.events.open(uri, token)).toMatchObject({ uri, standing: { listed: false } });

			const open = await t.events.create(host, party, input());
			expect(await t.events.open(open, null)).toMatchObject({ uri: open });
			expect(await t.events.open(`${open}x`, null)).toBeNull();
		});

		it('resets the link for someone with events.manage or staff, so the old one stops working', async () => {
			const uri = await t.events.create(host, party, input({ audience: 'private' }));
			const token = async () => (await t.events.get(uri))?.record.linkToken ?? '';
			const first = await token();

			await t.events.resetLink(host, uri);
			const second = await token();
			expect(second).toMatch(/^[\w-]{43}$/);
			expect(second).not.toBe(first);
			expect(await t.events.open(uri, first)).toBeNull();
			expect(await t.events.open(uri, second)).toMatchObject({ uri });

			await t.events.resetLink(await t.viewer(ADMIN), uri);
			expect(stored(uri).value.linkToken).not.toBe(second);
			// Who reset it is on record, newest first, and the tokens never are.
			expect(await audited('event.resetLink')).toEqual([
				{ actorRole: 'admin', summary: 'Reset the link to Winter Opening', changes: [] },
				{ actorRole: 'member', summary: 'Reset the link to Winter Opening', changes: [] }
			]);

			await expect(t.events.resetLink(await t.viewer(DOOR), uri)).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.events.resetLink(host, await t.events.create(host, party, input()))).rejects.toThrow(
				new UserError('Only a private event has a link to reset')
			);
		});
	});

	it('saves over the version opened, marking a moved start rescheduled and dropping what was cleared', async () => {
		const uri = await t.events.create(
			host,
			party,
			input({ tickets: 'https://tickets.example/a', audience: 'private' })
		);

		const first = stored(uri);

		await t.events.update(host, uri, { ...input({ name: 'Winter Opening II', audience: 'private' }), swap: first.cid });
		expect(stored(uri).value).toMatchObject({ name: 'Winter Opening II', status: EVENT_STATUS.scheduled });
		expect(stored(uri).value.tickets).toBeUndefined();
		// The link stays the same while it stays private.
		expect(stored(uri).value.linkToken).toBe(first.value.linkToken);

		await expect(t.events.update(host, uri, { ...input(), swap: first.cid })).rejects.toThrow(
			new UserError(STALE_PAGE, 409)
		);

		await t.events.update(host, uri, { ...input({ start: '22:00' }), swap: stored(uri).cid });
		expect(stored(uri).value).toMatchObject({ status: EVENT_STATUS.rescheduled, audience: 'public', createdBy: HOST });
		expect(stored(uri).value.linkToken).toBeUndefined();

		await expect(
			t.events.update(await t.viewer(DOOR), uri, { ...input(), swap: stored(uri).cid })
		).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.events.update(host, `${uri}x`, { ...input(), swap: 'bafy' })).rejects.toBeInstanceOf(NotFoundError);
		await expect(t.events.update(host, venue.space, { ...input(), swap: 'bafy' })).rejects.toBeInstanceOf(
			NotFoundError
		);
	});

	it("logs a staff save with each field's before and after, but never the link token", async () => {
		const uri = await t.events.create(host, party, input());

		await t.events.update(await t.viewer(ADMIN), uri, {
			...input({ name: 'Winter Closing', audience: 'private' }),
			swap: stored(uri).cid
		});

		expect(await audited('event.update')).toEqual([
			{
				actorRole: 'admin',
				summary: 'Updated Winter Closing',
				changes: [
					{ field: 'audience', before: 'public', after: 'private' },
					{ field: 'name', before: 'Winter Opening', after: 'Winter Closing' }
				]
			}
		]);
	});

	it('cancels an event, which stays with the party as cancelled through later edits', async () => {
		const uri = await t.events.create(host, party, input());

		await t.events.cancel(host, uri, stored(uri).cid);
		expect(stored(uri).value.status).toBe(EVENT_STATUS.cancelled);

		await t.events.update(host, uri, { ...input({ start: '21:00' }), swap: stored(uri).cid });
		expect((await t.events.get(uri))?.record.status).toBe(EVENT_STATUS.cancelled);
	});

	it("doesn't index an event or an approval that isn't valid", async () => {
		const invalid = { name: '', status: EVENT_STATUS.scheduled };

		await t.sharedSpaces.index({ space: party, collection: COLLECTION.event, rkey: '3e1', value: invalid });
		await t.sharedSpaces.index({
			space: venue.space,
			collection: COLLECTION.venueApproval,
			rkey: '3a1',
			value: invalid
		});

		expect(await t.db.selectFrom('event').select('uri').execute()).toEqual([]);
		expect(await t.db.selectFrom('venueApproval').select('rkey').execute()).toEqual([]);
	});

	describe('at the venue', () => {
		it('lists it at once waiting for the venue by default, and Confirmed once someone with venue.approve approves', async () => {
			const uri = await t.events.create(host, party, input());

			expect(await t.events.get(uri)).toMatchObject({ approved: false, standing: { listed: true, confirmed: false } });
			expect((await t.events.waiting(venue.space)).map((event) => event.uri)).toEqual([uri]);

			await expect(t.events.approve(host, uri)).rejects.toBeInstanceOf(ForbiddenError);
			await t.events.approve(await t.viewer(BOOKER), uri);

			expect(await t.events.get(uri)).toMatchObject({ approved: true, standing: { listed: true, confirmed: true } });
			expect(await t.events.waiting(venue.space)).toEqual([]);
			const approvals = await t.db.selectFrom('venueApproval').selectAll().execute();
			expect(approvals).toEqual([
				expect.objectContaining({ spaceUri: venue.space, eventUri: uri, approvedBy: BOOKER })
			]);
			expect(await audited('event.approve')).toEqual([
				{ actorRole: 'member', summary: 'Confirmed Winter Opening', changes: [] }
			]);

			// Approving again changes nothing.
			await t.events.approve(await t.viewer(OWNER), uri);
			expect(await t.db.selectFrom('venueApproval').selectAll().execute()).toHaveLength(1);
			await expect(t.events.approve(await t.viewer(OWNER), `${uri}x`)).rejects.toBeInstanceOf(NotFoundError);
		});

		it('lists an event only once Confirmed under Venue approves', async () => {
			await t.venues.setApproval(await t.viewer(OWNER), venue.space, 'approval');
			const uri = await t.events.create(host, party, input());

			expect((await t.events.get(uri))?.standing).toEqual({ listed: false, confirmed: false });
			await t.events.approve(await t.viewer(OWNER), uri);
			expect((await t.events.get(uri))?.standing).toEqual({ listed: true, confirmed: true });
		});

		it('never confirms an event from a blocked party, which looks like one still waiting for the venue', async () => {
			await t.venues.block(
				await t.viewer(OWNER),
				venue.space,
				`at://${ROOT}/${COLLECTION.listing}/${party.split('/').at(-1)}`
			);
			const uri = await t.events.create(host, party, input());
			const waiting = (await t.events.get(uri))?.standing;

			await t.events.approve(await t.viewer(OWNER), uri);

			expect((await t.events.get(uri))?.standing).toEqual(waiting);
			expect(waiting).toEqual({ listed: true, confirmed: false });
		});

		it('never confirms an event with a blocked performer on the lineup', async () => {
			await t.venues.block(await t.viewer(OWNER), venue.space, act);
			const uri = await t.events.create(host, party, input({ lineup: [{ performer: act }] }));

			await t.events.approve(await t.viewer(OWNER), uri);

			expect((await t.events.get(uri))?.standing).toEqual({ listed: true, confirmed: false });
		});

		it('leaves cancelled and past events out of what is waiting for the venue', async () => {
			const cancelled = await t.events.create(host, party, input());
			await t.events.cancel(host, cancelled, stored(cancelled).cid);
			await t.events.create(host, party, input({ date: '2026-11-01' }));
			const next = await t.events.create(host, party, input({ date: '2026-11-20' }));
			const later = await t.events.create(host, party, input({ date: '2027-01-09' }));

			expect((await t.events.waiting(venue.space)).map((event) => event.uri)).toEqual([next, later]);
		});

		it('calls an event still waiting for the venue at 6:00 after its night never confirmed, off every list and closed to visitors', async () => {
			const waiting = await t.events.create(host, party, input({ date: '2026-11-13', end: undefined }));
			// Starting at 2:00, it's still the evening before's night, which ends the same morning.
			const late = await t.events.create(host, party, input({ date: '2026-11-14', start: '02:00', end: undefined }));
			const confirmed = await t.events.create(host, party, input({ date: '2026-11-13', end: undefined }));
			await t.events.approve(await t.viewer(OWNER), confirmed);
			const uris = async (events: Promise<{ uri: string }[]>) => (await events).map((event) => event.uri).toSorted();

			// 5:59 in Berlin.
			vi.setSystemTime(new Date('2026-11-14T04:59:00Z'));
			expect(await uris(t.events.waiting(venue.space))).toEqual([waiting, late].toSorted());
			expect(await uris(t.events.listed())).toEqual([waiting, late, confirmed].toSorted());
			expect(await t.events.get(waiting)).toMatchObject({ over: false, neverConfirmed: false });

			vi.setSystemTime(new Date('2026-11-14T05:00:00Z'));

			for (const uri of [waiting, late]) {
				expect(await t.events.get(uri)).toMatchObject({
					over: true,
					neverConfirmed: true,
					standing: { listed: false, confirmed: false }
				});
				expect(await t.events.open(uri, null)).toBeNull();
			}

			expect(await t.events.waiting(venue.space)).toEqual([]);
			expect(await t.events.listed()).toEqual([]);
			// Confirmed, it's just a past event, which still opens.
			expect(await t.events.open(confirmed, null)).toMatchObject({ over: true, neverConfirmed: false });
		});

		it("ends a venue-less draft's night on its own time zone's clock, and never calls a draft never confirmed", async () => {
			const zoned = (date: string) => ({ ...bare, date, start: '23:00', timeZone: 'America/New_York' });
			const past = await t.events.create(host, party, zoned('2026-11-12'));
			const tonight = await t.events.create(host, party, zoned('2026-11-13'));

			// 6:00 on 13 November in New York is 11:00 UTC, an hour before now; tonight's ends on the 14th.
			expect(await t.events.get(past)).toMatchObject({ over: true, neverConfirmed: false });
			expect(await t.events.get(tonight)).toMatchObject({ over: false, neverConfirmed: false });
		});

		it('lists published public events soonest first by the instant they start, at a venue or anywhere', async () => {
			const admin = await t.viewer(ADMIN);

			const elsewhere = await t.venues.add(admin, {
				name: 'Nowadays',
				address: { country: 'US', locality: 'New York' },
				geo: { latitude: '40.7128', longitude: '-74.006' }
			});

			const owner = await t.viewer(OWNER);
			await t.sharedSpaces.addPerson(admin, elsewhere.space, { did: OWNER, role: 'owner' });
			await t.venues.setApproval(owner, elsewhere.space, 'approval');
			// 20:00 in New York is 2:00 in Berlin, after Tresor's 23:00, though it sorts first as text.
			const newYork = await t.events.create(host, party, input({ venue: elsewhere.space, start: '20:00' }));
			const berlin = await t.events.create(host, party, input());
			const cancelled = await t.events.create(host, party, input({ date: '2026-12-12' }));
			await t.events.cancel(host, cancelled, stored(cancelled).cid);
			await t.events.create(host, party, input({ audience: 'private' }));
			await t.events.create(host, party, input({ publish: false }));
			// Under Venue approves, waiting for the venue isn't listed.
			await t.events.create(host, party, input({ venue: elsewhere.space, date: '2026-12-06' }));
			await t.events.approve(owner, newYork);

			expect((await t.events.listed()).map((event) => event.uri)).toEqual([berlin, newYork, cancelled]);
			expect((await t.events.listed(venue.space)).map((event) => event.uri)).toEqual([berlin, cancelled]);
		});
	});
});
