import { createHash } from 'node:crypto';
import { TID } from '@atproto/common-web';
import type { Kysely } from 'kysely';
import type { NotificationChannel, NotificationTopic, NotificationView, PausedReason } from '#lib/types.ts';
import { backoffMs } from '../audit/log';
import { type Db, nowIso, parseJson } from '../db';
import type { Database, NotificationChannelRow } from '../db/schema';
import type { Logger } from '../logger';
import type { NotificationChannels } from './channels';
import { view } from './notifier';

/** What happened to one message. A sender returns it rather than throwing. */
export type SendOutcome =
	/** It went. */
	| { kind: 'sent'; providerId: string | null }
	/** It didn't, but might next time (the service was down or busy). */
	| { kind: 'retryable'; error: string; retryAfterMs?: number }
	/** The address can't take it, so nothing more goes there until `reason` is fixed. */
	| { kind: 'rejected'; reason: PausedReason; error: string }
	/** The service refused it for a reason trying again won't change, like the site's own setup. */
	| { kind: 'failed'; error: string };

export type Recipient = Pick<NotificationChannelRow, 'did' | 'address' | 'unsubscribeToken'>;

/** Sends inbox items by one channel. */
export type ChannelSender = {
	/**
	 * How long an item waits for others to go out with it, which is also the least time between two messages to one
	 * person, so a busy thread sends a few messages rather than one per reply.
	 */
	windowMs: number;
	/** Sends `items` to one person in one message. `key` is the same each time the same message is tried. */
	send(to: Recipient, items: NotificationView[], key: string): Promise<SendOutcome>;
};

/** What a test message says. */
const TEST_ITEM: Omit<NotificationView, 'occurredAt'> = {
	id: 'test',
	topic: null,
	title: 'This is a test from Local Network',
	body: 'Updates from your inbox will look like this.',
	href: '/dashboard/inbox',
	read: false
};

/** How long after it happened something still goes out by email or DM. Older ones (a slow sync) stay on the site. */
const DELIVER_MS = 6 * 60 * 60_000;

/** The most items in one message. */
const BATCH_SIZE = 20;

/** How long a claimed message can be on its way before a restart counts it as cut off and tries it again. */
const STALE_MS = 2 * 60_000;

/** How long finished deliveries are kept, for looking into a complaint. */
const KEEP_MS = 30 * 86_400_000;

const CHANNELS: NotificationChannel[] = ['email'];

type Due = {
	id: string;
	notificationId: string;
	did: string;
	channel: NotificationChannel;
	batchId: string | null;
	attempts: number;
	readAt: string | null;
};

/**
 * The outbox that sends inbox items on by email or DM, for people who opted in. Items queue with the item itself and
 * go out from here in the background, never on the request that made them. Each message holds the items due for one
 * person and channel; once claimed, its items and key stay the same through every retry, so a service that dedupes on
 * the key never sends it twice.
 */
export class Outbox {
	private timer: NodeJS.Timeout | undefined;
	private running: Promise<void> | undefined;

	constructor(
		private readonly deps: {
			db: Db;
			logger: Logger;
			channels: NotificationChannels;
			senders: Partial<Record<NotificationChannel, ChannelSender>>;
			maxAttempts?: number;
		}
	) {}

	/** The channels this site can send by. */
	get configured(): NotificationChannel[] {
		return CHANNELS.filter((channel) => this.deps.senders[channel]);
	}

	/** Queues new inbox items about `topic` for each recipient's active channels that send it, if it's recent enough. */
	async enqueue(
		db: Kysely<Database>,
		created: { id: string; recipientDid: string }[],
		topic: NotificationTopic,
		at: string
	): Promise<number> {
		const channels = this.configured;

		if (created.length === 0 || channels.length === 0 || !(Date.now() - Date.parse(at) < DELIVER_MS)) return 0;

		const active = await db
			.selectFrom('notificationChannel')
			.selectAll()
			.where(
				'did',
				'in',
				created.map((row) => row.recipientDid)
			)
			.where('channel', 'in', channels)
			.where('status', '=', 'active')
			.execute();

		const now = Date.now();
		const updatedAt = nowIso();

		const deliveries = active
			.filter((channel) => parseJson<string[]>(channel.topics, []).includes(topic))
			.flatMap((channel) => {
				const windowMs = this.deps.senders[channel.channel]?.windowMs ?? 0;
				const last = channel.lastSentAt ? Date.parse(channel.lastSentAt) : 0;
				const nextAttemptAt = new Date(Math.max(now, last) + windowMs).toISOString();

				return created.flatMap((row) =>
					row.recipientDid !== channel.did
						? []
						: [
								{
									id: TID.nextStr(),
									notificationId: row.id,
									did: channel.did,
									channel: channel.channel,
									status: 'pending' as const,
									batchId: null,
									attempts: 0,
									nextAttemptAt,
									lastError: null,
									providerId: null,
									sentAt: null,
									updatedAt
								}
							]
				);
			});

		if (deliveries.length > 0) await db.insertInto('notificationDelivery').values(deliveries).execute();

		return deliveries.length;
	}

	/** Has the outbox send what's due when the next item comes due. */
	async kick(): Promise<void> {
		const next = await this.deps.db
			.selectFrom('notificationDelivery')
			.select((eb) => eb.fn.min('nextAttemptAt').as('at'))
			.where('status', '=', 'pending')
			.executeTakeFirst();

		clearTimeout(this.timer);

		if (!next?.at) return;
		this.timer = setTimeout(() => void this.processDue(), Math.max(0, Date.parse(next.at) - Date.now()));
		this.timer.unref();
	}

	/** Sends everything due. A call while a run is going joins it. */
	processDue(): Promise<void> {
		this.running ??= this.run()
			.catch((error) => this.deps.logger.error({ err: error }, 'notification delivery crashed'))
			.finally(() => {
				this.running = undefined;
			});

		return this.running;
	}

	/**
	 * Sends `did` a test message by `channel` right away, outside the outbox. Returns how it went, or null when the
	 * channel isn't on and working for them.
	 */
	async sendTest(did: string, channel: NotificationChannel): Promise<SendOutcome | null> {
		const sender = this.deps.senders[channel];
		const recipient = await this.deps.channels.get(did, channel);

		if (!sender || recipient?.status !== 'active') return null;
		const outcome = await sender.send(recipient, [{ ...TEST_ITEM, occurredAt: nowIso() }], `test-${TID.nextStr()}`);

		if (outcome.kind === 'rejected') await this.deps.channels.pause(did, channel, outcome.reason);
		this.deps.logger.info({ did, channel, outcome: outcome.kind }, 'sent a test notification');

		return outcome;
	}

	/** Waits for a run that's going to finish. */
	async idle(): Promise<void> {
		await this.running;
	}

	stop(): void {
		clearTimeout(this.timer);
	}

	/**
	 * Puts back messages a crash or restart cut off while they were being sent, to go again under the same key. Run
	 * once at startup.
	 */
	async recover(): Promise<number> {
		const result = await this.deps.db
			.updateTable('notificationDelivery')
			.set({ status: 'pending', updatedAt: nowIso() })
			.where('status', '=', 'sending')
			.where('updatedAt', '<', new Date(Date.now() - STALE_MS).toISOString())
			.executeTakeFirst();

		const recovered = Number(result.numUpdatedRows);

		if (recovered > 0) this.deps.logger.warn({ recovered }, 'retrying notifications cut off mid-send');

		return recovered;
	}

	/** Deletes finished deliveries older than the outbox keeps them. */
	async prune(now = Date.now()): Promise<number> {
		const result = await this.deps.db
			.deleteFrom('notificationDelivery')
			.where('status', 'in', ['sent', 'failed', 'skipped'])
			.where('updatedAt', '<', new Date(now - KEEP_MS).toISOString())
			.executeTakeFirst();

		return Number(result.numDeletedRows);
	}

	private async run(): Promise<void> {
		const due = await this.deps.db
			.selectFrom('notificationDelivery as delivery')
			.innerJoin('notification', 'notification.id', 'delivery.notificationId')
			.select([
				'delivery.id',
				'delivery.notificationId',
				'delivery.did',
				'delivery.channel',
				'delivery.batchId',
				'delivery.attempts',
				'notification.readAt'
			])
			.where('delivery.status', '=', 'pending')
			.where('delivery.nextAttemptAt', '<=', nowIso())
			.orderBy('delivery.nextAttemptAt')
			.limit(500)
			.execute();

		for (const batch of this.batches(due)) await this.send(batch);
	}

	/**
	 * Splits what's due into messages: a message claimed before goes again as it was, and new items go together by
	 * person and channel, leaving out any already read on the site.
	 */
	private batches(due: Due[]): Due[][] {
		const groups = new Map<string, Due[]>();

		for (const row of due) {
			const key = row.batchId ?? `${row.did}|${row.channel}`;
			groups.set(key, [...(groups.get(key) ?? []), row]);
		}

		return [...groups.values()].flatMap((rows) => {
			if (rows[0].batchId) return [rows];
			const chunks: Due[][] = [];

			for (let start = 0; start < rows.length; start += BATCH_SIZE) chunks.push(rows.slice(start, start + BATCH_SIZE));

			return chunks;
		});
	}

	private async send(batch: Due[]): Promise<void> {
		const { db, logger } = this.deps;
		const [{ did, channel }] = batch;
		const sender = this.deps.senders[channel];
		const recipient = await this.deps.channels.get(did, channel);
		const fresh = !batch[0].batchId;
		// Read on the site already, or the channel's off, paused, or no longer set up here: nothing to send.
		const skipped = batch.filter((row) => !sender || recipient?.status !== 'active' || (fresh && row.readAt));
		const sending = batch.filter((row) => !skipped.includes(row));

		if (skipped.length > 0) await this.finish(skipped, { status: 'skipped' });

		if (!sender || !recipient || sending.length === 0) return;
		const ids = sending.map((row) => row.id);
		const batchId = sending[0].batchId ?? createHash('sha256').update(ids.toSorted().join(',')).digest('hex');
		const now = nowIso();

		const claimed = await db
			.updateTable('notificationDelivery')
			.set((eb) => ({ status: 'sending', batchId, attempts: eb('attempts', '+', 1), updatedAt: now }))
			.where('id', 'in', ids)
			.where('status', '=', 'pending')
			.executeTakeFirst();

		// Another run took them first.
		if (Number(claimed.numUpdatedRows) !== ids.length) return;

		const notifications = await db
			.selectFrom('notification')
			.selectAll()
			.where(
				'id',
				'in',
				sending.map((row) => row.notificationId)
			)
			.orderBy('occurredAt')
			.execute();

		let outcome: SendOutcome;

		try {
			outcome = await sender.send(recipient, notifications.map(view), batchId);
		} catch (error) {
			// Senders report failures as outcomes; anything thrown is a bug or a dropped connection, so it's tried again.
			outcome = { kind: 'retryable', error: String(error) };
		}

		const attempts = sending[0].attempts + 1;
		const log = { did, channel, batchId, items: ids.length, attempts };

		switch (outcome.kind) {
			case 'sent':
				await this.finish(sending, { status: 'sent', sentAt: now, providerId: outcome.providerId });
				await db
					.updateTable('notificationChannel')
					.set({ lastSentAt: now })
					.where('did', '=', did)
					.where('channel', '=', channel)
					.execute();
				logger.info(log, 'sent notifications');

				return;
			case 'retryable': {
				if (attempts >= (this.deps.maxAttempts ?? 8)) {
					await this.finish(sending, { status: 'failed', lastError: outcome.error });
					logger.error({ ...log, error: outcome.error }, 'gave up sending notifications');

					return;
				}

				const wait = Math.max(backoffMs(attempts), outcome.retryAfterMs ?? 0);
				const nextAttemptAt = new Date(Date.now() + wait).toISOString();
				await this.finish(sending, { status: 'pending', nextAttemptAt, lastError: outcome.error });
				logger.warn({ ...log, error: outcome.error, wait }, 'could not send notifications; trying again');

				return;
			}

			case 'rejected':
				await this.finish(sending, { status: 'failed', lastError: outcome.error });
				await this.deps.channels.pause(did, channel, outcome.reason);
				logger.warn({ ...log, error: outcome.error }, 'the address refused notifications');

				return;
			case 'failed':
				await this.finish(sending, { status: 'failed', lastError: outcome.error });
				logger.error({ ...log, error: outcome.error }, 'could not send notifications');
		}
	}

	private async finish(
		rows: Due[],
		update: {
			status: 'pending' | 'sent' | 'failed' | 'skipped';
			nextAttemptAt?: string;
			lastError?: string;
			sentAt?: string;
			providerId?: string | null;
		}
	): Promise<void> {
		await this.deps.db
			.updateTable('notificationDelivery')
			.set({ ...update, updatedAt: nowIso() })
			.where(
				'id',
				'in',
				rows.map((row) => row.id)
			)
			.execute();
	}
}
