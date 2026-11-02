import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { NotificationView } from '#lib/types.ts';
import { type Db, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { RateLimiter } from '../rate-limit';
import { NotificationChannels } from './channels';
import { Notifier, type NotifyInput } from './notifier';
import { type ChannelSender, Outbox, type Recipient, type SendOutcome } from './outbox';

const MINUTE = 60_000;

const [neon, remy] = ['did:plc:neon', 'did:plc:remy'];

const thread = 'at://did:plc:root/space/computer.localnetwork.booking/3mabc';

const sent: SendOutcome = { kind: 'sent', providerId: 'msg_1' };

/** An email channel that records each message and answers with the next of `outcomes`, then `sent`. */
class FakeSender implements ChannelSender {
	readonly windowMs = 2 * MINUTE;
	readonly messages: { to: Recipient; titles: string[]; key: string }[] = [];
	outcomes: (SendOutcome | Error)[] = [];

	async send(to: Recipient, items: NotificationView[], key: string): Promise<SendOutcome> {
		this.messages.push({ to, titles: items.map((item) => item.title), key });
		const outcome = this.outcomes.shift() ?? sent;

		if (outcome instanceof Error) throw outcome;

		return outcome;
	}
}

describe('Outbox', () => {
	let db: Db;
	let channels: NotificationChannels;
	let sender: FakeSender;
	let outbox: Outbox;
	let inbox: Notifier;

	const message = (key: string, overrides: Partial<NotifyInput<'booking.message'>> = {}) =>
		inbox.notify({
			kind: 'booking.message',
			to: [neon, remy],
			key,
			subject: thread,
			data: { skey: '3mabc', title: key, projectName: 'Neon Tide', from: 'promoter' },
			href: '/dashboard/bookings/3mabc',
			at: new Date().toISOString(),
			...overrides
		});

	const later = (ms: number) => vi.setSystemTime(Date.now() + ms);

	const statuses = async () =>
		(await db.selectFrom('notificationDelivery').select('status').orderBy('id').execute()).map((row) => row.status);

	/** Turns email on for `did`, as signing in with the permission and opening the confirmation link do. */
	const optIn = async (did: string) => {
		const email = `${did.split(':').at(-1)}@example.test`;
		let link = '';

		channels = new NotificationChannels({
			db,
			logger: silentLogger,
			sendConfirmation: async (_address, token) => {
				link = token;
			},
			limits: {
				account: new RateLimiter(1, 1),
				accountAddress: new RateLimiter(1, 1),
				address: new RateLimiter(1, 1),
				all: new RateLimiter(1, 1)
			}
		});
		await channels.onSignIn({
			did,
			scope: 'atproto account:email',
			optIn: true,
			session: async () => ({ email, emailConfirmed: true })
		});
		await channels.confirmEmail(link);
	};

	beforeEach(async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		db = await openDatabase(':memory:');
		await optIn(neon);
		sender = new FakeSender();
		outbox = new Outbox({ db, logger: silentLogger, channels, senders: { email: sender }, maxAttempts: 3 });
		inbox = new Notifier({ db, logger: silentLogger, outbox });
	});

	afterEach(async () => {
		outbox.stop();
		await outbox.idle();
		vi.useRealTimers();
	});

	it('sends a burst as one email once it settles, only to those who turned email on', async () => {
		await message('first');
		await message('second');
		await outbox.processDue();
		expect(sender.messages).toEqual([]);

		later(2 * MINUTE);
		await outbox.processDue();
		expect(sender.messages).toEqual([
			{
				to: expect.objectContaining({ did: neon, address: 'neon@example.test' }),
				titles: ['The promoter sent a message', 'The promoter sent a message'],
				key: expect.any(String)
			}
		]);
		expect(await statuses()).toEqual(['sent', 'sent']);

		// The next one waits out the window after the last email, too.
		await message('third');
		later(MINUTE);
		await outbox.processDue();
		expect(sender.messages).toHaveLength(1);
		later(MINUTE);
		await outbox.processDue();
		expect(sender.messages).toHaveLength(2);
	});

	it('keeps to the inbox what happened hours ago, topics turned off, and channels that are paused', async () => {
		await message('old', { at: new Date(Date.now() - 7 * 60 * MINUTE).toISOString() });
		await db.updateTable('notificationChannel').set({ topics: '["events"]' }).execute();
		await message('off topic');
		await db.updateTable('notificationChannel').set({ topics: '["bookings"]' }).execute();
		await channels.pause(neon, 'email', 'address_rejected');
		await message('paused');

		expect(await statuses()).toEqual([]);
		expect(await inbox.unreadCount(neon)).toBe(3);
	});

	it("skips what's been read on the site before it went, and what's queued for a channel since turned off", async () => {
		await message('read');
		await inbox.markAllRead(neon);
		later(2 * MINUTE);
		await outbox.processDue();
		expect(await statuses()).toEqual(['skipped']);

		await message('turned off');
		await channels.disable(neon, 'email');
		later(2 * MINUTE);
		await outbox.processDue();
		expect(await statuses()).toEqual(['skipped', 'skipped']);
		expect(sender.messages).toEqual([]);
	});

	it('tries the same email again under the same key, waiting longer each time, then gives up', async () => {
		sender.outcomes = [{ kind: 'retryable', error: 'busy', retryAfterMs: 30_000 }, new Error('socket hang up')];
		await message('flaky');
		later(2 * MINUTE);
		await outbox.processDue();
		// What arrives meanwhile goes in an email of its own.
		await message('meanwhile');

		later(20_000);
		await outbox.processDue();
		expect(sender.messages).toHaveLength(1);
		later(10_000);
		await outbox.processDue();
		later(10_000);
		await outbox.processDue();
		later(2 * MINUTE);
		await outbox.processDue();

		const [first, second, third, other] = sender.messages;
		expect([second.key, third.key]).toEqual([first.key, first.key]);
		expect(third.titles).toEqual(first.titles);
		expect(other.key).not.toBe(first.key);
		expect(await statuses()).toEqual(['sent', 'sent']);

		sender.outcomes = [1, 2, 3].map(() => ({ kind: 'retryable', error: 'down' }));
		await message('never');

		for (const wait of [2 * MINUTE, MINUTE, MINUTE]) {
			later(wait);
			await outbox.processDue();
		}

		expect(await statuses()).toEqual(['sent', 'sent', 'failed']);
	});

	it('pauses email for an address that refuses it, and leaves it on when the site is at fault', async () => {
		sender.outcomes = [{ kind: 'failed', error: 'domain not verified' }];
		await message('misconfigured');
		later(2 * MINUTE);
		await outbox.processDue();
		expect((await channels.get(neon, 'email'))?.status).toBe('active');

		sender.outcomes = [{ kind: 'rejected', reason: 'address_rejected', error: 'invalid to' }];
		await message('rejected');
		later(2 * MINUTE);
		await outbox.processDue();
		expect(await channels.get(neon, 'email')).toMatchObject({ status: 'paused', pausedReason: 'address_rejected' });
		expect(await statuses()).toEqual(['failed', 'failed']);

		await message('after');
		expect(await statuses()).toHaveLength(2);
	});

	it('sends again what a restart cut off mid-send, as the same email', async () => {
		await message('cut off');
		later(2 * MINUTE);
		await db
			.updateTable('notificationDelivery')
			.set({ status: 'sending', batchId: 'batch-1', updatedAt: new Date().toISOString() })
			.execute();

		expect(await outbox.recover()).toBe(0);
		later(3 * MINUTE);
		expect(await outbox.recover()).toBe(1);
		await outbox.processDue();
		expect(sender.messages.map((sent) => sent.key)).toEqual(['batch-1']);
	});

	it('sends as soon as something comes due, without waiting for the next round', async () => {
		vi.useRealTimers();
		const quick = Object.assign(new FakeSender(), { windowMs: 0 });
		outbox = new Outbox({ db, logger: silentLogger, channels, senders: { email: quick } });
		inbox = new Notifier({ db, logger: silentLogger, outbox });

		await message('now');
		await vi.waitFor(() => expect(quick.messages).toHaveLength(1));
		await outbox.kick();
		expect(new Outbox({ db, logger: silentLogger, channels, senders: {} }).configured).toEqual([]);
	});

	it('sends a test right away to someone with email on, and pauses an address that refuses it', async () => {
		expect(await outbox.sendTest(neon, 'email')).toEqual(sent);
		expect(sender.messages).toEqual([
			{
				to: expect.objectContaining({ did: neon }),
				titles: ['This is a test from Local Network'],
				key: expect.stringMatching(/^test-/)
			}
		]);
		expect(await outbox.sendTest(remy, 'email')).toBeNull();

		sender.outcomes = [{ kind: 'rejected', reason: 'address_rejected', error: 'invalid to' }];
		expect(await outbox.sendTest(neon, 'email')).toMatchObject({ kind: 'rejected' });
		expect((await channels.get(neon, 'email'))?.pausedReason).toBe('address_rejected');
		expect(await outbox.sendTest(neon, 'email')).toBeNull();
	});

	it('sends by every channel someone turned on, each on its own clock', async () => {
		const dm = Object.assign(new FakeSender(), { windowMs: MINUTE });
		outbox = new Outbox({ db, logger: silentLogger, channels, senders: { email: sender, dm } });
		inbox = new Notifier({ db, logger: silentLogger, outbox });
		await channels.enableDm(neon, true);
		expect(outbox.configured).toEqual(['email', 'dm']);

		await message('both');
		later(MINUTE);
		await outbox.processDue();
		expect([sender.messages.length, dm.messages.length]).toEqual([0, 1]);
		expect(dm.messages[0].to).toMatchObject({ did: neon, address: null });

		later(MINUTE);
		await outbox.processDue();
		expect([sender.messages.length, dm.messages.length]).toEqual([1, 1]);
	});

	it('keeps finished deliveries for 30 days', async () => {
		await message('done');
		later(2 * MINUTE);
		await outbox.processDue();

		expect(await outbox.prune(Date.now() + 29 * 1440 * MINUTE)).toBe(0);
		expect(await outbox.prune(Date.now() + 31 * 1440 * MINUTE)).toBe(1);
	});
});
