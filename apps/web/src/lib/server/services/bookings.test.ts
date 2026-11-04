import { XRPCError } from '@atproto/api';
import { TID } from '@atproto/common-web';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { memberSpacePolicy } from '../atproto/repo-client';
import { bookingSpace, spaceRecordUri } from '../atproto/spaces';
import { NotFoundError } from '../core';
import { HappyViewError } from '../happyview/client';
import { createTestServices, ROOT } from '../testing/core';
import { FakeRootClient } from '../testing/fakes';
import type { BookingRecord } from './bookings';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

const PROMOTER = 'did:plc:promoter';

/** An account whose PDS can't write to spaces, so the root writes for it. */
const LEGACY = 'did:plc:legacy';

const OUTSIDER = 'did:plc:outsider';

const profile = (name: string): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }
});

describe('BookingService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ, PROMOTER, OUTSIDER] });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await t.audit.idle();
		await t.db.destroy();
	});

	const bookable = async () =>
		t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Bookable'), managers: [DJ] });

	const open = async (actor = PROMOTER) =>
		t.bookings.start({
			actor: await t.viewer(actor),
			projectUri: (await bookable()).uri,
			show: { title: 'Night' },
			message: 'Free?'
		});

	/** A booking space the root made elsewhere, with these members, that this server has no thread for yet. */
	const syncedSpace = async (members: string[]) => {
		const root = t.net.client(ROOT);

		const space = await root.createSpace({
			spaceType: SPACE_TYPE.booking,
			skey: TID.nextStr(),
			policy: memberSpacePolicy()
		});

		for (const did of members) await root.putMember({ space, did, read: true, write: true });

		return space;
	};

	/** Indexes a record `repo` wrote in `space`, as a sync of it would. */
	const synced = (space: string, repo: string, collection: string, value: BookingRecord) =>
		t.bookings.indexEvent({
			space,
			uri: spaceRecordUri(space, repo, collection, TID.nextStr()),
			repoDid: repo,
			collection,
			value
		});

	const titles = async (did: string) => (await t.notifications.list(did)).map((item) => item.title);

	it("refuses inquiries to a missing project or without a message, and threads it doesn't have", async () => {
		const promoter = await t.viewer(PROMOTER);
		const project = await bookable();

		const ask = (projectUri: string, message: string) =>
			t.bookings.start({ actor: promoter, projectUri, show: { title: 'x' }, message });

		await expect(ask(`at://${ROOT}/${COLLECTION.project}/3missing`, 'hi')).rejects.toThrow(NotFoundError);
		await expect(ask(project.uri, '   ')).rejects.toThrow('Write a message');

		const space = await ask(project.uri, 'hi');
		await expect(t.bookings.message({ actor: promoter, space, text: '  ' })).rejects.toThrow('Write a message');
		await expect(
			t.bookings.message({ actor: promoter, space: bookingSpace(ROOT, 'gone'), text: 'hello?' })
		).rejects.toThrow(NotFoundError);
		expect(await t.bookings.bySkey('gone')).toBeNull();
	});

	it('writes for a promoter without spaces and names them on every record it writes for them', async () => {
		const space = await open(LEGACY);
		await t.bookings.setStatus({ actor: await t.viewer(LEGACY), space, status: 'cancelled', note: 'Plans changed' });

		const onBehalf = t.net
			.spaceRecordsIn(space, ROOT)
			.map((record) => record.value)
			.filter((value) => value.onBehalfOf === LEGACY);

		expect(onBehalf.map((value) => value.$type)).toEqual([COLLECTION.bookingInquiry, COLLECTION.bookingStatus]);

		const { thread, events } = await t.bookings.thread(await t.viewer(LEGACY), space);
		expect(thread).toMatchObject({ status: 'cancelled', promoter: { did: LEGACY }, lastAuthor: LEGACY });
		expect(events.map((event) => [event.kind, event.author.did, event.text])).toEqual([
			['inquiry', LEGACY, 'Free?'],
			['status', LEGACY, 'Plans changed']
		]);
	});

	it("opens a thread it hears of only from sync, ignoring records from anyone outside it, for a project that's gone", async () => {
		const space = await syncedSpace([PROMOTER, DJ]);
		const createdAt = new Date().toISOString();

		await synced(space, OUTSIDER, COLLECTION.bookingInquiry, { project: 'x', message: 'Let me in', createdAt });
		await synced(space, PROMOTER, 'app.bsky.feed.post', { text: 'Not a booking record', createdAt });
		await t.bookings.indexEvent({
			space,
			uri: 'not a record uri',
			repoDid: PROMOTER,
			collection: COLLECTION.bookingMessage,
			value: { text: 'Lost', createdAt }
		});
		expect(await t.bookings.threadsFor(PROMOTER)).toEqual([]);

		// An inquiry without a show, naming a project the directory no longer has, so no manager joins it.
		await synced(space, PROMOTER, COLLECTION.bookingInquiry, {
			project: `at://${ROOT}/${COLLECTION.project}/3gone`,
			message: 'Free?',
			createdAt
		});

		expect(await t.bookings.threadsFor(PROMOTER)).toMatchObject([
			{
				space,
				title: null,
				project: null,
				promoter: { did: PROMOTER },
				participants: [{ did: PROMOTER, role: 'promoter' }],
				lastAuthor: PROMOTER
			}
		]);
		expect(await t.bookings.threadsFor(DJ)).toEqual([]);
		expect(await t.bookings.threadsFor(OUTSIDER)).toEqual([]);
	});

	it("drops a synced record while the space's members can't be read, and takes it on the next sync", async () => {
		const space = await syncedSpace([PROMOTER]);

		const inquiry = {
			project: `at://${ROOT}/${COLLECTION.project}/3gone`,
			message: 'Free?',
			createdAt: new Date().toISOString()
		};

		vi.spyOn(FakeRootClient.prototype, 'listMembers').mockRejectedValueOnce(new XRPCError(502, 'UpstreamFailure'));

		await synced(space, PROMOTER, COLLECTION.bookingInquiry, inquiry);
		expect(await t.bookings.threadsFor(PROMOTER)).toEqual([]);

		await synced(space, PROMOTER, COLLECTION.bookingInquiry, inquiry);
		expect((await t.bookings.threadsFor(PROMOTER)).map((thread) => thread.space)).toEqual([space]);
	});

	it("still tells the promoter the act wrote when the directory can't name the act", async () => {
		const space = await open();
		vi.spyOn(t.directory, 'query').mockRejectedValue(new HappyViewError(503, 'down'));

		await t.bookings.message({ actor: await t.viewer(DJ), space, text: 'Yes' });

		expect(await titles(PROMOTER)).toEqual(['The DJ sent a message']);
	});

	it('keeps a synced status record that names no status without changing the thread or telling anyone', async () => {
		const space = await open();
		const before = await titles(PROMOTER);

		await synced(space, DJ, COLLECTION.bookingStatus, { createdAt: new Date().toISOString() });

		expect((await t.bookings.thread(await t.viewer(PROMOTER), space)).thread).toMatchObject({
			status: 'open',
			lastAuthor: DJ
		});
		expect(await titles(PROMOTER)).toEqual(before);
	});

	it('forgets a thread whose space is gone, with its records', async () => {
		const space = await open();
		const promoter = await t.viewer(PROMOTER);
		const { thread } = await t.bookings.thread(promoter, space);

		await t.bookings.forget(space);

		expect(await t.bookings.bySkey(thread.skey)).toBeNull();
		expect(await t.bookings.threadsFor(PROMOTER)).toEqual([]);
		await expect(t.bookings.thread(promoter, space)).rejects.toThrow(NotFoundError);
		expect(await t.db.selectFrom('bookingEvent').where('spaceUri', '=', space).selectAll().execute()).toEqual([]);
	});
});
