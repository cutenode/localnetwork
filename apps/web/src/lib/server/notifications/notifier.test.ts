import { beforeEach, describe, expect, it } from 'vitest';
import { ZodError } from 'zod';
import { type Db, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { type NotifyInput, Notifier } from './notifier';

const DAY_MS = 86_400_000;

const ago = (ms: number) => new Date(Date.now() - ms).toISOString();

const [pat, neon, remy] = ['did:plc:pat', 'did:plc:neon', 'did:plc:remy'];

const thread = 'at://did:plc:root/space/computer.localnetwork.booking/3mabc';

const message = (overrides: Partial<NotifyInput<'booking.message'>> = {}): NotifyInput<'booking.message'> => ({
	kind: 'booking.message',
	to: [neon, remy],
	key: `${thread}/${pat}/computer.localnetwork.booking.message/1`,
	subject: thread,
	data: { skey: '3mabc', title: 'Warehouse night', projectName: 'Neon Tide', from: 'promoter' },
	href: '/dashboard/bookings/3mabc',
	at: ago(60_000),
	...overrides
});

describe('Notifier', () => {
	let db: Db;
	let inbox: Notifier;

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		inbox = new Notifier({ db, logger: silentLogger });
	});

	it("tells each recipient once per thing that happened, in words read from the item's kind", async () => {
		const input = message();
		expect(await inbox.notify(input)).toBe(2);
		// Indexing the same record again, from sync say, tells nobody twice.
		expect(await inbox.notify(message({ to: [neon, remy, neon] }))).toBe(0);
		expect(await inbox.list(pat)).toEqual([]);

		expect(await inbox.list(neon)).toEqual([
			{
				id: expect.any(String),
				topic: 'bookings',
				title: 'The promoter sent a message',
				body: 'Warehouse night',
				href: '/dashboard/bookings/3mabc',
				occurredAt: input.at,
				read: false
			}
		]);
		expect(await inbox.notify(message({ to: [] }))).toBe(0);
	});

	it('words each booking kind from the side that acted, naming the act and never who in it', async () => {
		const data = { skey: '3mabc', title: null, projectName: 'Neon Tide', from: 'act' } as const;
		await inbox.notify({
			...message({ key: 'inquiry', data: { ...data, from: 'promoter' } }),
			kind: 'booking.inquiry'
		});
		await inbox.notify({
			...message({ key: 'nameless', data: { ...data, projectName: null, from: 'promoter' } }),
			kind: 'booking.inquiry'
		});
		await inbox.notify(message({ key: 'reply', data }));
		await inbox.notify(message({ key: 'reply, unlisted', data: { ...data, projectName: null } }));
		await inbox.notify({
			...message({ key: 'status' }),
			kind: 'booking.status',
			data: { ...data, status: 'confirmed' }
		});

		const items = await inbox.list(neon);

		expect(items.map(({ title, body }) => ({ title, body }))).toEqual(
			expect.arrayContaining([
				{ title: 'New booking inquiry for Neon Tide', body: 'Booking inquiry' },
				{ title: 'New booking inquiry', body: 'Booking inquiry' },
				{ title: 'Neon Tide sent a message', body: 'Booking inquiry' },
				{ title: 'The DJ sent a message', body: 'Booking inquiry' },
				{ title: 'Neon Tide marked the booking confirmed', body: 'Booking inquiry' }
			])
		);
		expect(items).toHaveLength(5);
	});

	it('leaves out what happened too long ago to be news, like a re-sync of old records', async () => {
		expect(await inbox.notify(message({ at: ago(15 * DAY_MS) }))).toBe(0);
		expect(await inbox.notify(message({ at: 'not a date' }))).toBe(0);
		expect(await inbox.notify(message({ at: ago(13 * DAY_MS) }))).toBe(2);
	});

	it('refuses data that does not fit the kind, and links only within the site', async () => {
		// SAFETY: deliberately not one of the sides the kind allows, as a producer with a bug would pass.
		const from = 'manager' as 'act';
		await expect(
			inbox.notify(message({ data: { skey: '3mabc', title: null, projectName: null, from } }))
		).rejects.toThrow(ZodError);

		await inbox.notify(message({ href: 'https://elsewhere.example/phish' }));
		expect((await inbox.list(neon))[0].href).toBe('/dashboard/inbox');
	});

	it('shows items it no longer knows how to word as something that changed, still linking there', async () => {
		await inbox.notify(message());
		await db.updateTable('notification').set({ kind: 'event.retired' }).where('recipientDid', '=', neon).execute();
		await db.updateTable('notification').set({ data: '{"skey":1}' }).where('recipientDid', '=', remy).execute();

		for (const did of [neon, remy]) {
			expect(await inbox.list(did)).toEqual([
				expect.objectContaining({ topic: null, title: 'Something changed', href: '/dashboard/bookings/3mabc' })
			]);
		}
	});

	it('lists newest first, a page at a time', async () => {
		for (const minutes of [5, 4, 3, 2, 1]) {
			await inbox.notify(message({ key: `m${minutes}`, at: ago(minutes * 60_000) }));
		}

		const first = await inbox.list(neon, { limit: 2 });
		const second = await inbox.list(neon, { limit: 2, before: first.at(-1)?.id });
		const third = await inbox.list(neon, { limit: 2, before: second.at(-1)?.id });
		const times = [...first, ...second, ...third].map((item) => item.occurredAt);

		expect(times).toEqual(times.toSorted().reverse());
		expect(new Set(times).size).toBe(5);
		// Someone else's item is no place to start from.
		const theirs = (await inbox.list(remy))[0].id;
		expect(await inbox.list(neon, { limit: 2, before: theirs })).toEqual(first);
	});

	it("counts and marks only the recipient's own items read", async () => {
		await inbox.notify(message({ key: 'a' }));
		await inbox.notify(message({ key: 'b', subject: 'at://did:plc:root/space/computer.localnetwork.booking/other' }));
		await inbox.notify(message({ key: 'c' }));
		expect(await inbox.unreadCount(neon)).toBe(3);

		const [item] = await inbox.list(neon);
		expect(await inbox.open(remy, item.id)).toBeNull();
		expect(await inbox.open(neon, item.id)).toBe('/dashboard/bookings/3mabc');
		expect(await inbox.unreadCount(neon)).toBe(2);
		expect(await inbox.unreadCount(remy)).toBe(3);

		await inbox.markReadBySubject(neon, thread);
		expect(await inbox.unreadCount(neon)).toBe(1);
		expect(await inbox.unreadCount(remy)).toBe(3);

		await inbox.markAllRead(neon);
		expect(await inbox.unreadCount(neon)).toBe(0);
		expect((await inbox.list(neon)).every((listed) => listed.read)).toBe(true);
		expect(await inbox.unreadCount(remy)).toBe(3);
	});

	it('keeps read items for 90 days and unread ones for 180', async () => {
		await inbox.notify(message());
		await inbox.markAllRead(neon);

		expect(await inbox.prune(Date.now() + 89 * DAY_MS)).toBe(0);
		expect(await inbox.prune(Date.now() + 91 * DAY_MS)).toBe(1);
		expect(await inbox.unreadCount(remy)).toBe(1);
		expect(await inbox.prune(Date.now() + 181 * DAY_MS)).toBe(1);
		expect(await inbox.list(remy)).toEqual([]);
	});
});
