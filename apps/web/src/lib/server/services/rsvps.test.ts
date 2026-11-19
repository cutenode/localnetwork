import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CALENDAR_EVENT, CALENDAR_RSVP } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { recordCid } from '../atproto/repo-client';
import { ReauthError, UserError } from '../core';
import { HappyViewClient } from '../happyview/client';
import { createTestServices, PUBLIC_URL, ROOT } from '../testing/core';
import type { EventInput, EventView } from './events';
import { RsvpService } from './rsvps';
import type { VenueView } from './venues';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const FAN = 'did:plc:fan';

const FRIEND = 'did:plc:friend';

/** An account whose session can't write to its repo (signed out since, or a PDS that lost it). */
const LAPSED = 'did:plc:lapsed';

const GOING = `${CALENDAR_RSVP}#going`;

const INTERESTED = `${CALENDAR_RSVP}#interested`;

const missingScope = () => new XRPCError(403, 'ScopeMissingError', `Missing required scope "repo:${CALENDAR_RSVP}"`);

describe('RsvpService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let host: Viewer;
	let fan: Viewer;
	let party: string;
	let venue: VenueView;

	beforeEach(async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
		t = await createTestServices({ admins: [ADMIN], capable: [FAN, FRIEND] });
		host = await t.viewer(HOST);
		fan = await t.viewer(FAN);
		party = await t.parties.start(host, { basics: { name: 'Night Shift' }, avatar: 'keep', publish: true });
		venue = await t.venues.add(await t.viewer(ADMIN), {
			name: 'Tresor',
			address: { country: 'DE', locality: 'Berlin' },
			geo: { latitude: '52.51', longitude: '13.42' }
		});
	});
	afterEach(async () => {
		vi.useRealTimers();
		await t.audit.idle();
		await t.db.destroy();
	});

	/** A published public event, unless `overrides` say otherwise, as its page reads it. */
	const make = async (overrides: Partial<EventInput> = {}): Promise<EventView> => {
		const uri = await t.events.create(host, party, {
			name: 'Winter Opening',
			date: '2026-12-05',
			start: '23:00',
			end: '06:00',
			venue: venue.space,
			audience: 'public',
			publish: true,
			...overrides
		});

		return (await t.events.get(uri))!;
	};

	const rsvpsOf = (did: string) =>
		[...t.net.records.entries()].flatMap(([key, stored]) =>
			key.startsWith(`${did}|${CALENDAR_RSVP}|`) ? [stored] : []
		);

	/** Writes an RSVP the way another calendar app would, straight to `did`'s repo. */
	const elsewhere = (did: string, subject: { uri: string; cid: string }, status: string) =>
		t.net.client(did).createRecord({ collection: CALENDAR_RSVP, record: { $type: CALENDAR_RSVP, subject, status } });

	it("answers the event's calendar copy as it is now, from the guest's own repo, and takes the answer back", async () => {
		const event = await make();
		const copy = t.net.records.get(`${ROOT}|${CALENDAR_EVENT}|${event.rkey}`)!;
		const subject = { uri: `at://${ROOT}/${CALENDAR_EVENT}/${event.rkey}`, cid: copy.cid };

		expect(await t.rsvps.subjectFor(event)).toEqual(subject);
		expect(await t.rsvps.state(event, FAN)).toEqual({ going: 0, interested: 0, answer: null });

		await t.rsvps.answer(fan, event, 'going');
		const [going] = rsvpsOf(FAN);
		expect(going.value).toEqual({ $type: CALENDAR_RSVP, subject, status: GOING });
		expect(await t.rsvps.state(event, FAN)).toEqual({ going: 1, interested: 0, answer: 'going' });
		expect(await t.rsvps.state(event, undefined)).toEqual({ going: 1, interested: 0, answer: null });

		// Switching keeps the one record.
		await t.rsvps.answer(fan, event, 'interested');
		expect(rsvpsOf(FAN).map((stored) => [stored.uri, stored.value.status])).toEqual([[going.uri, INTERESTED]]);
		expect(await t.rsvps.state(event, FAN)).toEqual({ going: 0, interested: 1, answer: 'interested' });

		await t.rsvps.answer(fan, event, null);
		await t.rsvps.answer(fan, event, null);
		expect(rsvpsOf(FAN)).toEqual([]);
		expect(await t.rsvps.state(event, FAN)).toEqual({ going: 0, interested: 0, answer: null });
	});

	it('counts each guest once by their latest RSVP from any app, and leaves one once they answer here', async () => {
		const event = await make();
		const other = await make({ name: 'Winter Closing' });
		const subject = (await t.rsvps.subjectFor(event))!;

		// Someone who said going in another app, then not going.
		await elsewhere('did:plc:stranger', subject, GOING);
		await elsewhere('did:plc:stranger', subject, `${CALENDAR_RSVP}#notgoing`);
		await elsewhere(FRIEND, subject, INTERESTED);
		await elsewhere(FAN, subject, INTERESTED);
		await elsewhere(FAN, subject, GOING);
		// Nobody keeps RSVPs to anyone else's calendar events.
		await elsewhere(FAN, { uri: `at://did:plc:else/${CALENDAR_EVENT}/${event.rkey}`, cid: subject.cid }, GOING);

		expect(await t.rsvps.state(event, FAN)).toEqual({ going: 1, interested: 1, answer: 'going' });
		expect(await t.rsvps.state(event, 'did:plc:stranger')).toEqual({ going: 1, interested: 1, answer: null });
		expect(await t.rsvps.state(other, FAN)).toEqual({ going: 0, interested: 0, answer: null });

		await t.rsvps.answer(fan, event, 'interested');
		const answers = rsvpsOf(FAN).filter((stored) => JSON.stringify(stored.value.subject) === JSON.stringify(subject));
		expect(answers.map((stored) => stored.value.status)).toEqual([INTERESTED]);
		expect(await t.rsvps.state(event, FAN)).toEqual({ going: 0, interested: 2, answer: 'interested' });
	});

	it('points at the copy as it is after a save', async () => {
		const event = await make();
		const before = await t.rsvps.subjectFor(event);

		await t.events.update(host, event.uri, {
			name: 'Winter Opening',
			date: '2026-12-05',
			start: '22:00',
			venue: venue.space,
			audience: 'public',
			publish: true,
			swap: event.cid
		});
		const after = await t.rsvps.subjectFor((await t.events.get(event.uri))!);

		expect(after).toEqual({
			uri: before?.uri,
			cid: await recordCid(t.net.records.get(`${ROOT}|${CALENDAR_EVENT}|${event.rkey}`)!.value)
		});
		expect(after?.cid).not.toBe(before?.cid);
	});

	it('takes no answers for a draft, a private event, a cancelled one or one whose night is over', async () => {
		const refused = new UserError("This event isn't taking answers");
		const cancelled = await make();
		await t.events.cancel(host, cancelled.uri, cancelled.cid);
		// Confirmed, so it's still listed once its night is over.
		const past = await make();
		await t.events.approve(await t.viewer(ADMIN), past.uri);

		const events = [
			await make({ publish: false }),
			await make({ audience: 'private' }),
			(await t.events.get(cancelled.uri))!
		];

		for (const event of events) await expect(t.rsvps.answer(fan, event, 'going')).rejects.toThrow(refused);
		expect(await t.rsvps.state(events[0], FAN)).toBeNull();
		expect(await t.rsvps.state(events[1], FAN)).toBeNull();

		vi.setSystemTime(new Date('2026-12-06T05:00:00Z'));
		await expect(t.rsvps.answer(fan, (await t.events.get(past.uri))!, 'going')).rejects.toThrow(refused);
		expect(rsvpsOf(FAN)).toEqual([]);
	});

	it("takes none for an event the venue doesn't list until it confirms it", async () => {
		await t.venues.setApproval(await t.viewer(ADMIN), venue.space, 'approval');
		const event = await make();

		expect(await t.rsvps.state(event, FAN)).toBeNull();
		await expect(t.rsvps.answer(fan, event, 'going')).rejects.toThrow("This event isn't taking answers");

		await t.events.approve(await t.viewer(ADMIN), event.uri);
		await t.rsvps.answer(fan, (await t.events.get(event.uri))!, 'going');
		expect(await t.rsvps.state((await t.events.get(event.uri))!, FAN)).toMatchObject({ going: 1 });
	});

	it('asks to sign in again when the session is gone or lacks the scope, and writes nothing', async () => {
		const event = await make();

		await expect(t.rsvps.answer(await t.viewer(LAPSED), event, 'going')).rejects.toThrow(ReauthError);
		t.net.failWrites(FAN, 1, missingScope());
		await expect(t.rsvps.answer(fan, event, 'going')).rejects.toMatchObject({
			status: 401,
			message: "Sign in again to say you're going"
		});
		expect(rsvpsOf(FAN)).toEqual([]);
	});

	/** An RsvpService over a directory that answers every query with `body`. */
	const answering = (body: string) =>
		new RsvpService(
			t.core,
			new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response(body) }),
			PUBLIC_URL,
			0
		);

	it('passes on a directory answer that is not JSON, rather than calling answers unavailable', async () => {
		await expect(answering('<html>').answer(fan, await make(), 'going')).rejects.toThrow(SyntaxError);
		expect(rsvpsOf(FAN)).toEqual([]);
	});

	it('deletes nothing for an RSVP the directory names without a record key', async () => {
		const event = await make();
		await t.rsvps.answer(fan, event, 'going');
		const rsvps = [{ uri: `at://${FAN}/${CALENDAR_RSVP}`, status: GOING }];

		await answering(JSON.stringify({ going: 1, interested: 0, rsvps })).answer(fan, event, null);
		expect(rsvpsOf(FAN)).toHaveLength(1);
	});

	it("says answers aren't available while the directory is down", async () => {
		const event = await make();
		const down = new HappyViewClient({ url: 'http://happyview.test', fetch: () => Promise.reject(new Error('down')) });

		await expect(new RsvpService(t.core, down, PUBLIC_URL, 0).answer(fan, event, 'going')).rejects.toMatchObject({
			status: 503,
			message: "Going and Interested aren't available right now. Try again in a minute."
		});
	});

	it("has HappyView re-read the guest's RSVPs when it doesn't serve a new one in time", async () => {
		const event = await make();
		const backfills: unknown[] = [];

		const directory = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'key',
			fetch: async (resource, init) => {
				const request = new Request(resource, init);

				if (new URL(request.url).pathname !== '/admin/backfill') return t.happyView.fetch(request);
				backfills.push(await request.json());

				return Response.json({ id: 'job', status: 'running' }, { status: 201 });
			}
		});

		// HappyView never hears of the write, as when Jetstream drops it.
		t.net.onRecord = async () => {};

		await new RsvpService(t.core, directory, PUBLIC_URL, 0).answer(fan, event, 'going');
		expect(backfills).toEqual([{ collection: CALENDAR_RSVP, dids: [FAN] }]);
	});
});
