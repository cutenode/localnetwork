import { randomBytes } from 'node:crypto';
import type { NotificationChannel, NotificationTopic, PausedReason } from '#lib/types.ts';
import { grantsEmail } from '../atproto/oauth';
import { type Db, nowIso } from '../db';
import type { NotificationChannelRow } from '../db/schema';
import type { Logger } from '../logger';

/** Every topic, which a channel sends on until someone narrows it. */
export const TOPICS: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

/** What a session's `com.atproto.server.getSession` says about the account's email, with the permission to read it. */
export type SessionEmail = { email?: string; emailConfirmed?: boolean };

/**
 * Where each person's inbox is also sent. The inbox on the site always gets everything; a row here sends it on by
 * one channel too, and deleting the row turns that off and forgets the address.
 */
export class NotificationChannels {
	constructor(private readonly deps: { db: Db; logger: Logger }) {}

	get(did: string, channel: NotificationChannel): Promise<NotificationChannelRow | undefined> {
		return this.deps.db
			.selectFrom('notificationChannel')
			.selectAll()
			.where('did', '=', did)
			.where('channel', '=', channel)
			.executeTakeFirst();
	}

	/**
	 * Reads the account's email address on sign-in when the grant allows it, for someone turning email on (`optIn`) or
	 * who has it on, so a changed or newly confirmed address is picked up. Says 'declined' when they were turning it on
	 * and didn't allow it. A host that won't say leaves email as it was rather than failing the sign-in.
	 */
	async onSignIn(input: {
		did: string;
		scope: string;
		optIn: boolean;
		session: () => Promise<SessionEmail>;
	}): Promise<'declined' | 'read' | 'skipped'> {
		const { did, optIn } = input;

		if (!grantsEmail(input.scope, did)) return optIn ? 'declined' : 'skipped';

		if (!optIn && !(await this.get(did, 'email'))) return 'skipped';

		try {
			await this.readEmail(did, await input.session(), optIn);

			return 'read';
		} catch (error) {
			this.deps.logger.warn({ err: error, did }, 'could not read the email address');

			return 'skipped';
		}
	}

	/**
	 * Keeps the email address a sign-in with the permission to read it found, for someone who turned email on
	 * (`optIn`) or had it on already. An address their host hasn't confirmed pauses email until it has. Returns the
	 * channel as it is now, or undefined when email stays off.
	 */
	private async readEmail(
		did: string,
		session: SessionEmail,
		optIn: boolean
	): Promise<NotificationChannelRow | undefined> {
		const { db } = this.deps;
		const existing = await this.get(did, 'email');

		if (!existing && !optIn) return undefined;
		const now = nowIso();
		const address = session.email ?? null;
		const confirmed = Boolean(address && session.emailConfirmed);

		// An address the email service refused stays paused until it changes, or they turn email on again.
		const stillRejected = !optIn && existing?.pausedReason === 'address_rejected' && existing.address === address;

		const pausedReason: PausedReason | null = !confirmed
			? 'email_unconfirmed'
			: stillRejected
				? 'address_rejected'
				: null;

		const values = {
			status: pausedReason ? ('paused' as const) : ('active' as const),
			pausedReason,
			address,
			addressReadAt: now,
			updatedAt: now
		};

		await db
			.insertInto('notificationChannel')
			.values({
				did,
				channel: 'email',
				...values,
				unsubscribeToken: randomBytes(32).toString('base64url'),
				topics: JSON.stringify(TOPICS),
				lastSentAt: null,
				createdAt: now
			})
			.onConflict((oc) => oc.columns(['did', 'channel']).doUpdateSet(values))
			.execute();
		this.deps.logger.info({ did, status: values.status, pausedReason }, 'read the email address for the inbox');

		return this.get(did, 'email');
	}

	/** Stops sending by `channel` until something fixes `reason`. */
	async pause(did: string, channel: NotificationChannel, reason: PausedReason): Promise<void> {
		await this.deps.db
			.updateTable('notificationChannel')
			.set({ status: 'paused', pausedReason: reason, updatedAt: nowIso() })
			.where('did', '=', did)
			.where('channel', '=', channel)
			.execute();
		this.deps.logger.info({ did, channel, reason }, 'paused a notification channel');
	}

	/** Turns a channel off, forgetting its address. */
	async disable(did: string, channel: NotificationChannel): Promise<void> {
		await this.deps.db
			.deleteFrom('notificationChannel')
			.where('did', '=', did)
			.where('channel', '=', channel)
			.execute();
	}

	/** Whether `token` is an email unsubscribe link that still turns something off. */
	async hasToken(token: string): Promise<boolean> {
		return Boolean(await this.byToken(token));
	}

	/** Turns email off from an unsubscribe link. Returns whether the link turned anything off. */
	async unsubscribe(token: string): Promise<boolean> {
		const row = await this.byToken(token);

		if (!row) return false;
		await this.disable(row.did, 'email');
		this.deps.logger.info({ did: row.did }, 'turned email off from an unsubscribe link');

		return true;
	}

	private byToken(token: string) {
		return this.deps.db
			.selectFrom('notificationChannel')
			.select('did')
			.where('unsubscribeToken', '=', token)
			.executeTakeFirst();
	}
}
