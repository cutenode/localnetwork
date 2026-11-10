import { XRPCError } from '@atproto/api';
import { TID } from '@atproto/common-web';
import fc from 'fast-check';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { bookingActions, bookingGroup, type BookingRole } from '#lib/bookings.ts';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { memberSpacePolicy } from '../atproto/repo-client';
import { bookingSpace, parseSpaceUri, spaceRecordUri } from '../atproto/spaces';
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
		vi.useRealTimers();
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

	it('opens a thread on the second read of an inquiry the directory failed on the first', async () => {
		const space = await syncedSpace([PROMOTER, DJ]);
		const project = await bookable();

		const change = {
			space,
			uri: spaceRecordUri(space, PROMOTER, COLLECTION.bookingInquiry, TID.nextStr()),
			repoDid: PROMOTER,
			collection: COLLECTION.bookingInquiry,
			value: { project: project.uri, message: 'Free?', createdAt: new Date().toISOString() }
		};

		vi.spyOn(t.projects, 'byUri').mockRejectedValueOnce(new Error('directory offline'));
		await expect(t.bookings.indexEvent(change)).rejects.toThrow('directory offline');
		expect(await t.bookings.threadsFor(PROMOTER)).toEqual([]);

		await t.bookings.indexEvent(change);
		expect(await t.bookings.threadsFor(PROMOTER)).toMatchObject([{ space, promoter: { did: PROMOTER } }]);
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

	it('lists threads without their project while the directory is down', async () => {
		await open();
		vi.spyOn(t.directory, 'query').mockRejectedValue(new HappyViewError(503, 'down'));

		const [thread] = await t.bookings.threadsFor(PROMOTER);

		expect(thread.project).toBeNull();
	});

	it("still tells the promoter the act wrote when the directory can't name the act", async () => {
		const space = await open();
		vi.spyOn(t.directory, 'query').mockRejectedValue(new HappyViewError(503, 'down'));

		await t.bookings.message({ actor: await t.viewer(DJ), space, text: 'Yes' });

		expect(await titles(PROMOTER)).toEqual(['The DJ sent a message']);
	});

	it('takes a reply as the last word over a message dated ahead of it', async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime('2026-11-08T12:00:00.000Z');
		const space = await open();
		await synced(space, DJ, COLLECTION.bookingMessage, { text: 'See you', createdAt: '2027-11-08T12:00:00.000Z' });
		vi.setSystemTime('2026-11-08T12:01:00.000Z');

		await t.bookings.message({ actor: await t.viewer(PROMOTER), space, text: 'Great' });

		const { thread, events } = await t.bookings.thread(await t.viewer(PROMOTER), space);
		expect(bookingGroup(thread, PROMOTER)).toBe('active');
		expect(events.map((event) => event.text)).toEqual(['Free?', 'See you', 'Great']);
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

	describe('status records from sync', () => {
		const statusRecord = (space: string, repo: string, status: string, createdAt = new Date().toISOString()) =>
			synced(space, repo, COLLECTION.bookingStatus, { status, createdAt });

		const threadOf = async (space: string) => t.bookings.thread(await t.viewer(PROMOTER), space);

		const statusOf = async (space: string) => (await threadOf(space)).thread.status;

		const statusEvents = async (space: string) =>
			(await threadOf(space)).events.filter((event) => event.status).map((event) => event.status);

		it("won't let either side's client reopen or change a settled thread", async () => {
			const space = await open();
			await t.bookings.setStatus({ actor: await t.viewer(DJ), space, status: 'declined' });
			const before = await titles(DJ);

			for (const status of ['open', 'negotiating', 'confirmed', 'cancelled']) {
				await statusRecord(space, PROMOTER, status);
				await statusRecord(space, DJ, status);
			}

			expect(await statusOf(space)).toBe('declined');
			expect(await statusEvents(space)).toEqual(['declined']);
			expect(await titles(DJ)).toEqual(before);
		});

		it('takes only the moves a side has, as a direct change would', async () => {
			const space = await open();

			await statusRecord(space, PROMOTER, 'negotiating');
			await statusRecord(space, PROMOTER, 'declined');
			await statusRecord(space, DJ, 'cancelled');
			await statusRecord(space, DJ, 'booked');
			expect(await statusOf(space)).toBe('open');

			await statusRecord(space, DJ, 'negotiating');
			expect(await statusOf(space)).toBe('negotiating');
			await statusRecord(space, DJ, 'negotiating');
			expect(await statusEvents(space)).toEqual(['negotiating']);
		});

		it("doesn't take a manager's refused record as their answer to an inquiry", async () => {
			const space = await open();

			await statusRecord(space, PROMOTER, 'confirmed');
			await statusRecord(space, DJ, 'cancelled');

			expect(await statusOf(space)).toBe('open');
		});

		it("doesn't let a promoter's record pass as one the root wrote for a manager", async () => {
			const space = await open();

			await synced(space, PROMOTER, COLLECTION.bookingStatus, {
				status: 'declined',
				onBehalfOf: DJ,
				createdAt: new Date().toISOString()
			});

			expect(await statusOf(space)).toBe('open');
		});

		it("doesn't count a refused record as a thread's last word or its latest activity", async () => {
			const space = await open();
			const { thread } = await threadOf(space);

			await statusRecord(space, DJ, 'cancelled', new Date(Date.now() + 1000).toISOString());

			expect((await threadOf(space)).thread).toMatchObject({
				lastAuthor: thread.lastAuthor,
				lastActivityAt: thread.lastActivityAt
			});
		});

		it("keeps a record's first version, however it's rewritten", async () => {
			const space = await open();
			const uri = spaceRecordUri(space, DJ, COLLECTION.bookingStatus, TID.nextStr());
			const createdAt = new Date().toISOString();

			const write = (status: string) =>
				t.bookings.indexEvent({
					space,
					uri,
					repoDid: DJ,
					collection: COLLECTION.bookingStatus,
					value: { status, createdAt }
				});

			await write('negotiating');
			await write('declined');

			expect(await statusOf(space)).toBe('negotiating');
			expect(await statusEvents(space)).toEqual(['negotiating']);
		});

		it('takes a change once, however often a sync reads it', async () => {
			const space = await open();
			const uri = spaceRecordUri(space, DJ, COLLECTION.bookingStatus, TID.nextStr());
			const value = { status: 'negotiating', createdAt: new Date().toISOString() };
			const change = { space, uri, repoDid: DJ, collection: COLLECTION.bookingStatus, value };

			await t.bookings.indexEvent(change);
			await t.bookings.indexEvent(change);
			await statusRecord(space, PROMOTER, 'confirmed');
			await t.bookings.indexEvent(change);

			expect(await statusOf(space)).toBe('confirmed');
			expect(await statusEvents(space)).toEqual(['negotiating', 'confirmed']);
		});

		it("isn't wedged by a record dated in the future", async () => {
			const space = await open();

			await synced(space, DJ, COLLECTION.bookingMessage, { text: 'Later', createdAt: '2099-01-01T00:00:00.000Z' });
			await t.bookings.setStatus({ actor: await t.viewer(DJ), space, status: 'negotiating' });
			await statusRecord(space, DJ, 'confirmed', '2099-01-02T00:00:00.000Z');
			await t.bookings.setStatus({ actor: await t.viewer(PROMOTER), space, status: 'cancelled' });

			const { thread } = await threadOf(space);
			expect(thread.status).toBe('cancelled');
			expect(thread.lastActivityAt <= new Date().toISOString()).toBe(true);
		});

		it('applies a status that reaches it before the inquiry opening its thread', async () => {
			const space = await syncedSpace([PROMOTER, DJ]);
			const project = await bookable();

			await statusRecord(space, DJ, 'declined', '2026-10-02T00:00:00.000Z');
			expect(await t.bookings.bySkey(parseSpaceUri(space)?.skey ?? '')).toBeNull();

			await synced(space, PROMOTER, COLLECTION.bookingInquiry, {
				project: project.uri,
				show: { title: 'Night' },
				message: 'Free?',
				createdAt: '2026-10-01T00:00:00.000Z'
			});

			expect((await threadOf(space)).thread).toMatchObject({
				status: 'declined',
				lastActivityAt: '2026-10-02T00:00:00.000Z'
			});
		});

		it('goes by date between rivals that wait for the same thing', async () => {
			const space = await syncedSpace([PROMOTER, DJ]);
			const project = await bookable();

			await statusRecord(space, PROMOTER, 'cancelled', '2026-10-03T00:00:00.000Z');
			await statusRecord(space, DJ, 'declined', '2026-10-02T00:00:00.000Z');
			await synced(space, PROMOTER, COLLECTION.bookingInquiry, {
				project: project.uri,
				show: { title: 'Night' },
				message: 'Free?',
				createdAt: '2026-10-01T00:00:00.000Z'
			});

			expect(await statusOf(space)).toBe('declined');
		});

		it('leaves a thread settled by a record read while another was being applied', async () => {
			const space = await open();
			const builder = t.db.transaction();
			const execute = builder.execute.bind(builder);

			vi.spyOn(builder, 'execute').mockImplementation(async (run) => {
				await statusRecord(space, PROMOTER, 'cancelled', new Date(Date.now() - 1000).toISOString());

				return execute(run);
			});
			vi.spyOn(t.db, 'transaction').mockReturnValueOnce(builder);

			await statusRecord(space, DJ, 'declined');

			expect(await statusOf(space)).toBe('cancelled');
			expect(await statusEvents(space)).toEqual(['cancelled']);
		});

		it("lets the root's opening record name the project, even read after the inquiry", async () => {
			const space = await syncedSpace([PROMOTER, DJ]);
			const named = await bookable();

			const real = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Real'),
				managers: [DJ]
			});

			await synced(space, PROMOTER, COLLECTION.bookingInquiry, {
				project: named.uri,
				show: { title: 'Night' },
				message: 'Free?',
				createdAt: '2026-10-01T00:00:00.000Z'
			});
			await synced(space, ROOT, COLLECTION.bookingStatus, {
				status: 'open',
				project: real.uri,
				promoter: PROMOTER,
				createdAt: '2026-10-01T00:00:00.000Z'
			});

			expect((await threadOf(space)).thread.project?.uri).toBe(real.uri);
		});

		it("keeps a status the root wrote for someone off their list of held records when it didn't count", async () => {
			const space = await open(LEGACY);

			await synced(space, ROOT, COLLECTION.bookingStatus, {
				status: 'negotiating',
				onBehalfOf: LEGACY,
				createdAt: new Date().toISOString()
			});

			const held = await t.heldRecords.list(await t.viewer(LEGACY));
			expect(held.map((record) => record.title)).toEqual(['Inquiry in Night']);
		});

		it("applies a promoter's confirmation that reaches it before the reply allowing it, and says so once", async () => {
			const space = await open();
			const replied = new Date();
			const confirmed = new Date(replied.getTime() + 1000);

			await statusRecord(space, PROMOTER, 'confirmed', confirmed.toISOString());
			expect(await statusOf(space)).toBe('open');

			await synced(space, DJ, COLLECTION.bookingMessage, { text: 'Yes', createdAt: replied.toISOString() });

			expect(await statusOf(space)).toBe('confirmed');
			expect((await titles(DJ)).filter((title) => title.includes('confirmed'))).toHaveLength(1);
		});

		it('never applies a waiting record from a manager who has left', async () => {
			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Bookable'),
				managers: [DJ, OUTSIDER]
			});

			const space = await t.bookings.start({
				actor: await t.viewer(PROMOTER),
				projectUri: project.uri,
				show: { title: 'Night' },
				message: 'Free?'
			});

			await statusRecord(space, DJ, 'cancelled');
			await t.db.deleteFrom('bookingParticipant').where('spaceUri', '=', space).where('did', '=', DJ).execute();
			await t.bookings.setStatus({ actor: await t.viewer(OUTSIDER), space, status: 'confirmed' });

			expect(await statusOf(space)).toBe('confirmed');
		});

		it('tells the side that lost a race, and logs it as denied', async () => {
			const space = await open();

			const results = await Promise.allSettled([
				t.bookings.setStatus({ actor: await t.viewer(DJ), space, status: 'declined' }),
				t.bookings.setStatus({ actor: await t.viewer(PROMOTER), space, status: 'cancelled' })
			]);

			await t.audit.idle();
			const entries = await t.audit.list({ action: 'booking.status' });

			expect(results.map((result) => result.status).toSorted()).toEqual(['fulfilled', 'rejected']);
			expect(entries.map((entry) => entry.outcome).toSorted()).toEqual(['denied', 'success']);
		});

		it('reaches the same status from any order a sync reads a valid thread in', async () => {
			const project = await bookable();

			/** A thread's status records as the sides could make them, each picked from what's open to it then. */
			const history = (picks: number[]) => {
				const moves: { role: BookingRole; status: string }[] = [];

				for (const pick of picks) {
					const status = moves.at(-1)?.status ?? 'open';

					const options = (['manager', 'promoter'] as const).flatMap((role) =>
						bookingActions(role, status, true).map((action) => ({ role, status: action.status }))
					);

					if (options.length === 0) break;
					moves.push(options[pick % options.length]);
				}

				return moves;
			};

			const arrival = fc.array(fc.nat(), { maxLength: 4 }).chain((picks) => {
				const moves = history(picks);
				const records = [...Array(moves.length + 2).keys()];

				return fc.record({
					moves: fc.constant(moves),
					order: fc.shuffledSubarray(records, { minLength: records.length })
				});
			});

			await fc.assert(
				fc.asyncProperty(arrival, async ({ moves, order }) => {
					const space = await syncedSpace([PROMOTER, DJ]);
					const at = (step: number) => new Date(Date.UTC(2026, 9, 1) + step * 1000).toISOString();

					const records: (() => Promise<void>)[] = [
						() =>
							synced(space, PROMOTER, COLLECTION.bookingInquiry, {
								project: project.uri,
								show: { title: 'Night' },
								message: 'Free?',
								createdAt: at(0)
							}),
						() => synced(space, DJ, COLLECTION.bookingMessage, { text: 'Hi', createdAt: at(1) }),
						...moves.map(
							({ role, status }, step) =>
								() =>
									statusRecord(space, role === 'manager' ? DJ : PROMOTER, status, at(step + 2))
						)
					];

					for (const index of order) await records[index]();

					expect(await statusOf(space)).toBe(moves.at(-1)?.status ?? 'open');
				}),
				{ numRuns: 25 }
			);
		});
	});
});
