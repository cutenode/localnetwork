import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { epkRequestSpace, parseSpaceRecordUri, spaceOfRecord } from '../atproto/spaces';
import { NotFoundError } from '../core';
import { RateLimiter } from '../rate-limit';
import { ROOT } from '../testing/core';
import { ADMIN, DJ, NO_SPACES, pressKitServices, STRANGER, values } from '../testing/press-kits';
import { HeldRecords, isHeldRecordKind } from './held-records';

/** Another account whose PDS can't write to spaces. */
const OTHER = 'did:plc:other';

const profile = (name: string): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE' }
});

describe('HeldRecords', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;

	beforeEach(async () => {
		f = await pressKitServices();
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	/** Whether the root's repo still holds `uri`. */
	const stored = (uri: string) => {
		const { collection, rkey } = parseSpaceRecordUri(uri)!;

		return f.t.net.spaceRecords.has(`${spaceOfRecord(uri)}|${ROOT}|${collection}|${rkey}`);
	};

	/**
	 * NO_SPACES's records, all written by the root: a submission, a thread with an inquiry, a message and a cancellation, a
	 * press kit request and a draft.
	 */
	async function fileEverything() {
		const viewer = await f.t.viewer(NO_SPACES);
		const epk = await f.publish(DJ);

		const request = await f.t.requests.submit({
			actor: viewer,
			kind: 'create',
			profile: profile('Proxy'),
			message: 'Hi mods'
		});

		const space = await f.t.bookings.start({
			actor: viewer,
			projectUri: f.project.uri,
			show: { title: 'Rooftop' },
			message: 'Free on the 5th?'
		});

		await f.t.bookings.message({ actor: viewer, space, text: 'Any news?' });
		await f.t.bookings.setStatus({ actor: viewer, space, status: 'cancelled', note: 'Found someone' });
		await f.t.epks.request(viewer, f.kit(epk), { message: 'For a festival' });
		await f.setManagers([DJ, NO_SPACES]);
		await f.t.epks.drafts.save(viewer, f.project, {
			rkey: epk.rkey,
			cid: epk.cid,
			values: values({ title: 'Clubs 2027' })
		});

		return { viewer, epk, request, space };
	}

	it('lists only what the root wrote for the viewer, by kind, with what they wrote in it', async () => {
		await fileEverything();
		// Someone else's, written by the root, and the viewer's own, written by themselves, aren't theirs to delete here.
		await f.t.requests.submit({ actor: await f.t.viewer(OTHER), kind: 'create', profile: profile('Elsewhere') });
		await f.t.requests.submit({ actor: await f.t.viewer(STRANGER), kind: 'create', profile: profile('Own repo') });

		const held = await f.t.heldRecords.list(await f.t.viewer(NO_SPACES));

		// Records written in the same millisecond can come in either order, so only the kinds' order counts.
		const shown = held.map(({ kind, title, excerpt }) => ({ kind, title, excerpt }));
		expect(shown.map((record) => record.kind)).toEqual(['request', 'booking', 'booking', 'epkRequest', 'epkDraft']);
		expect(shown.toSorted((a, b) => a.title.localeCompare(b.title))).toEqual([
			{ kind: 'booking', title: 'Inquiry in Rooftop', excerpt: 'Free on the 5th?' },
			{ kind: 'booking', title: 'Message in Rooftop', excerpt: 'Any news?' },
			{ kind: 'request', title: 'New project: Proxy', excerpt: 'Hi mods' },
			{ kind: 'epkDraft', title: 'Press kit draft for Neon Tide', excerpt: null },
			{ kind: 'epkRequest', title: 'Press kit request for Neon Tide', excerpt: 'For a festival' }
		]);
		expect(held.every((record) => parseSpaceRecordUri(record.uri)?.author === ROOT)).toBe(true);
		expect(await f.t.heldRecords.list(await f.t.viewer(STRANGER))).toEqual([]);
	});

	it('deletes a booking message from the root repo and its thread, and audits it', async () => {
		const { viewer, space } = await fileEverything();
		const message = (await f.t.heldRecords.list(viewer)).find((record) => record.title === 'Message in Rooftop')!;

		await f.t.heldRecords.delete(viewer, message.uri);

		expect(stored(message.uri)).toBe(false);
		const { events } = await f.t.bookings.thread(await f.t.viewer(DJ), space);
		expect(events.map((event) => event.text)).toEqual(['Free on the 5th?', 'Found someone']);
		await f.t.audit.idle();
		const [entry] = await f.t.audit.list({ actor: NO_SPACES, action: 'record.delete' });
		expect(entry).toMatchObject({
			subject: message.uri,
			subjectKind: 'booking',
			summary: 'Deleted a booking message filed on their behalf'
		});
	});

	it('takes a pending submission or press kit request out of the queue, as withdrawing does', async () => {
		const { viewer, request, epk } = await fileEverything();
		const kitRequest = (await f.t.epks.requests(epk))[0];
		await f.setManagers([DJ]);

		await f.t.heldRecords.delete(viewer, request.uri);
		await f.t.heldRecords.delete(viewer, kitRequest.uri);

		expect(await f.t.requests.list({ status: 'pending' })).toEqual([]);
		expect(await f.t.epks.requests(epk)).toEqual([]);
		expect(await f.t.epks.accessState(f.kit(epk), viewer)).toMatchObject({ kind: 'canAsk' });
		expect([stored(request.uri), stored(kitRequest.uri)]).toEqual([false, false]);
	});

	it("keeps a decided submission's row and decision, without what it said", async () => {
		const { viewer, request } = await fileEverything();
		await f.t.requests.decide({
			actor: await f.t.viewer(ADMIN),
			uri: request.uri,
			outcome: 'rejected',
			reason: 'Duplicate'
		});

		await f.t.heldRecords.delete(viewer, request.uri);

		expect(stored(request.uri)).toBe(false);
		const kept = await f.t.requests.get(request.uri);
		expect(kept).toMatchObject({
			status: 'rejected',
			decision: { reason: 'Duplicate' },
			record: { kind: 'create', onBehalfOf: NO_SPACES }
		});
		expect(kept?.deletedAt).toEqual(expect.any(String));
		expect(Object.keys(kept!.record)).not.toContain('profile');
		expect(Object.keys(kept!.record)).not.toContain('message');
		expect((await f.t.heldRecords.list(viewer)).map((record) => record.kind)).not.toContain('request');
		await expect(f.t.heldRecords.delete(viewer, request.uri)).rejects.toBeInstanceOf(NotFoundError);
	});

	it('keeps a declined press kit request declined, without its role or message', async () => {
		const { viewer, epk } = await fileEverything();
		await f.setManagers([DJ]);
		const [asked] = await f.t.epks.requests(epk);
		await f.t.epks.decide(await f.t.viewer(DJ), f.kit(epk), asked.uri, 'decline');

		await f.t.heldRecords.delete(viewer, asked.uri);

		expect(stored(asked.uri)).toBe(false);
		expect(await f.t.epks.accessState(f.kit(epk), viewer)).toEqual({ kind: 'declined' });
		expect(await f.t.epks.requests(epk)).toMatchObject([
			{ uri: asked.uri, status: 'declined', role: null, message: null }
		]);
		expect((await f.t.heldRecords.list(viewer)).map((record) => record.kind)).not.toContain('epkRequest');
	});

	it("leaves booking status changes, which set the thread's status for both sides", async () => {
		const { viewer, space } = await fileEverything();

		const status = f.t.net
			.spaceRecordsIn(space, ROOT)
			.find((record) => record.uri.includes(COLLECTION.bookingStatus) && record.value.onBehalfOf)!;

		await expect(f.t.heldRecords.delete(viewer, status.uri)).rejects.toBeInstanceOf(NotFoundError);

		expect(stored(status.uri)).toBe(true);
		expect((await f.t.bookings.thread(viewer, space)).thread.status).toBe('cancelled');
	});

	it('discards a draft the root wrote for a manager', async () => {
		const { viewer } = await fileEverything();
		const draft = (await f.t.heldRecords.list(viewer)).find((record) => record.kind === 'epkDraft')!;

		await f.t.heldRecords.delete(viewer, draft.uri);

		expect(stored(draft.uri)).toBe(false);
		expect((await f.t.epks.dashboard(f.project)).kits[0].drafts).toEqual([]);
	});

	it("refuses someone else's record, a record they wrote themselves, and anything else", async () => {
		const viewer = await f.t.viewer(NO_SPACES);

		const others = await f.t.requests.submit({
			actor: await f.t.viewer(OTHER),
			kind: 'create',
			profile: profile('Elsewhere')
		});

		const own = await f.t.requests.submit({
			actor: await f.t.viewer(STRANGER),
			kind: 'create',
			profile: profile('Own')
		});

		for (const uri of [others.uri, own.uri, 'at://did:plc:root/app.bsky.feed.post/1', 'not a uri']) {
			await expect(f.t.heldRecords.delete(viewer, uri)).rejects.toBeInstanceOf(NotFoundError);
		}

		expect(stored(others.uri)).toBe(true);
		expect(await f.t.requests.get(others.uri)).toMatchObject({ status: 'pending' });
	});

	it('checks the record itself, not only the index, before deleting', async () => {
		const others = await f.t.requests.submit({
			actor: await f.t.viewer(OTHER),
			kind: 'create',
			profile: profile('Elsewhere')
		});

		await f.t.db.updateTable('request').set({ requesterDid: NO_SPACES }).where('uri', '=', others.uri).execute();

		await expect(f.t.heldRecords.delete(await f.t.viewer(NO_SPACES), others.uri)).rejects.toBeInstanceOf(NotFoundError);

		expect(stored(others.uri)).toBe(true);
	});

	it('only drops the index row of a record that is gone already, with nothing to audit', async () => {
		const viewer = await f.t.viewer(NO_SPACES);
		const request = await f.t.requests.submit({ actor: viewer, kind: 'create', profile: profile('Proxy') });
		await f.t.net
			.client(ROOT)
			.deleteSpaceRecord({ space: f.t.spaces.moderation, collection: COLLECTION.request, rkey: request.rkey });

		await f.t.heldRecords.delete(viewer, request.uri);

		expect(await f.t.requests.get(request.uri)).toBeNull();
		await f.t.audit.idle();
		expect(await f.t.audit.list({ action: 'record.delete' })).toEqual([]);
	});

	it('deletes a whole kind at once, leaving the rest', async () => {
		const { viewer, space } = await fileEverything();

		expect(await f.t.heldRecords.deleteAll(viewer, 'booking')).toBe(2);

		const left = f.t.net.spaceRecordsIn(space, ROOT).filter((record) => record.value.onBehalfOf);
		expect(left.map((record) => parseSpaceRecordUri(record.uri)?.collection)).toEqual([COLLECTION.bookingStatus]);

		expect((await f.t.heldRecords.list(viewer)).map((record) => record.kind)).toEqual([
			'request',
			'epkRequest',
			'epkDraft'
		]);
		expect(f.t.net.spaceRecordsIn(epkRequestSpace(ROOT, f.project.rkey), ROOT)).toHaveLength(1);
	});

	it('limits how often someone deletes', async () => {
		const held = new HeldRecords(f.t.core, f.t.requests, f.t.projects, new RateLimiter(1, 0));
		const viewer = await f.t.viewer(NO_SPACES);

		expect(await held.deleteAll(viewer, 'request')).toBe(0);
		await expect(held.delete(viewer, 'not a uri')).rejects.toMatchObject({ status: 429, message: /Too many/ });
	});

	it('knows its kinds', () => {
		expect(['request', 'booking', 'epkRequest', 'epkDraft', 'toString', 'audit'].map(isHeldRecordKind)).toEqual([
			true,
			true,
			true,
			true,
			false,
			false
		]);
	});
});
