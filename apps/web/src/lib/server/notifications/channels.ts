import { createHash, randomBytes } from 'node:crypto';
import type { NotificationChannel, NotificationTopic, PausedReason } from '#lib/types.ts';
import { grantsEmail } from '../atproto/oauth';
import { type Db, nowIso } from '../db';
import type { NotificationChannelRow } from '../db/schema';
import type { Logger } from '../logger';
import type { RateLimiter } from '../rate-limit';

/** Every topic, which a channel sends on until someone narrows it. */
export const TOPICS: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

/** How long a link confirming an email address works. */
const CONFIRM_MS = 24 * 60 * 60_000;

/** Confirmation links are kept hashed: unlike the unsubscribe link, one turns email on. */
const hashToken = (token: string) => createHash('sha256').update(token).digest('hex');

/** What a session's `com.atproto.server.getSession` says about the account's email, with the permission to read it. */
export type SessionEmail = { email?: string; emailConfirmed?: boolean };

/**
 * Where each person's inbox is also sent. The inbox on the site always gets everything; a row here sends it on by
 * one channel too, and deleting the row turns that off and forgets the address.
 */
export class NotificationChannels {
	constructor(
		private readonly deps: {
			db: Db;
			logger: Logger;
			/** Emails `address` the link that confirms it, carrying `token`. Unset when the site can't send email. */
			sendConfirmation?: (address: string, token: string) => Promise<void>;
			/** How many confirmation emails one account can have sent. */
			confirmLimit: RateLimiter;
		}
	) {}

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
	 * (`optIn`) or had it on already. The site only sends to an address it confirmed itself, since a host can call any
	 * address confirmed: a new or changed one waits on the link in one confirmation email, which goes again when they
	 * turn email on again. An address their host hasn't confirmed either waits on that first. Returns the channel as it
	 * is now, or undefined when email stays off.
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
		const hostConfirmed = Boolean(address && session.emailConfirmed);

		const confirmedHere =
			existing?.address === address && (existing.status === 'active' || existing.pausedReason === 'address_rejected');

		// An address the email service refused stays paused until it changes, or they turn email on again.
		const rejected = confirmedHere && !optIn && existing.pausedReason === 'address_rejected';

		const pausedReason: PausedReason | null = !hostConfirmed
			? 'email_unconfirmed'
			: !confirmedHere
				? 'email_unverified'
				: rejected
					? 'address_rejected'
					: null;

		// A link goes out when the address first needs one (it's new, changed, or the host just confirmed it), or they ask.
		const firstNeeded = existing?.address !== address || existing.pausedReason !== 'email_unverified';
		const sendLink = pausedReason === 'email_unverified' && (optIn || firstNeeded);
		const token = sendLink && this.mayConfirm(did) ? randomBytes(32).toString('base64url') : null;

		const values = {
			status: pausedReason ? ('paused' as const) : ('active' as const),
			pausedReason,
			address,
			addressReadAt: now,
			updatedAt: now,
			// A link already out for this address keeps working until it's replaced or expires.
			...(token
				? { confirmTokenHash: hashToken(token), confirmExpiresAt: new Date(Date.now() + CONFIRM_MS).toISOString() }
				: pausedReason === 'email_unverified'
					? {}
					: { confirmTokenHash: null, confirmExpiresAt: null })
		};

		await db
			.insertInto('notificationChannel')
			.values({
				did,
				channel: 'email',
				confirmTokenHash: null,
				confirmExpiresAt: null,
				...values,
				unsubscribeToken: randomBytes(32).toString('base64url'),
				topics: JSON.stringify(TOPICS),
				lastSentAt: null,
				createdAt: now
			})
			.onConflict((oc) => oc.columns(['did', 'channel']).doUpdateSet(values))
			.execute();
		this.deps.logger.info({ did, status: values.status, pausedReason }, 'read the email address for the inbox');

		if (token && address) await this.deps.sendConfirmation?.(address, token);

		return this.get(did, 'email');
	}

	/** Whether `did` can be sent another confirmation email now. Asking past the limit leaves the last link working. */
	private mayConfirm(did: string): boolean {
		if (!this.deps.sendConfirmation) return false;

		if (this.deps.confirmLimit.take(`confirm:${did}`)) return true;
		this.deps.logger.warn({ did }, 'not sending another email confirmation yet');

		return false;
	}

	/** Whether a confirmation link still works, without using it. */
	async confirmation(token: string): Promise<'valid' | 'expired' | 'invalid'> {
		return (await this.byConfirmToken(token)).state;
	}

	/** Turns email on from a confirmation link, which then stops working. */
	async confirmEmail(token: string): Promise<'confirmed' | 'expired' | 'invalid'> {
		const found = await this.byConfirmToken(token);

		if (found.state !== 'valid') return found.state;
		const { row } = found;
		await this.deps.db
			.updateTable('notificationChannel')
			.set({
				status: 'active',
				pausedReason: null,
				confirmTokenHash: null,
				confirmExpiresAt: null,
				updatedAt: nowIso()
			})
			.where('did', '=', row.did)
			.where('channel', '=', 'email')
			.where('confirmTokenHash', '=', hashToken(token))
			.execute();
		this.deps.logger.info({ did: row.did }, 'confirmed an email address');

		return 'confirmed';
	}

	private async byConfirmToken(
		token: string
	): Promise<{ state: 'invalid' } | { state: 'valid' | 'expired'; row: { did: string } }> {
		const row = token
			? await this.deps.db
					.selectFrom('notificationChannel')
					.select(['did', 'pausedReason', 'confirmExpiresAt'])
					.where('channel', '=', 'email')
					.where('confirmTokenHash', '=', hashToken(token))
					.executeTakeFirst()
			: undefined;

		if (!row || row.pausedReason !== 'email_unverified') return { state: 'invalid' };

		return { row, state: !row.confirmExpiresAt || row.confirmExpiresAt < nowIso() ? 'expired' : 'valid' };
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
