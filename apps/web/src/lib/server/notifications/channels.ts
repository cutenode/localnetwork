import { createHash, randomBytes } from 'node:crypto';
import type { ChannelView, NotificationChannel, NotificationTopic, PausedReason } from '#lib/types.ts';
import { grantsEmail } from '../atproto/oauth';
import { type Db, nowIso, parseJson } from '../db';
import type { NotificationChannelRow } from '../db/schema';
import type { Logger } from '../logger';
import type { RateLimiter } from '../rate-limit';

/** Every topic, which a channel sends on until someone narrows it. */
export const TOPICS: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

/** How long a link confirming an email address works. */
const CONFIRM_MS = 24 * 60 * 60_000;

/** Confirmation links are kept hashed: unlike the unsubscribe link, one turns email on. */
const hashToken = (token: string) => createHash('sha256').update(token).digest('hex');

/** Which address a confirmation link went to, the same however its letters are cased. */
const hashAddress = (address: string) => createHash('sha256').update(address.toLowerCase()).digest('hex');

/**
 * Limits on confirmation emails, so the site can't be used to flood an address: per account, so one account changing
 * its address can't spend the overall allowance; per account and address; per address across every account; and
 * overall. Nothing holds an address for whoever asked first, so someone claiming another
 * person's address can't keep them from confirming it; the most they can do is use up the address's allowance, which
 * blocks its owner for a day at worst, while the same allowance caps what the address gets.
 */
export type ConfirmLimits = {
	account: RateLimiter;
	accountAddress: RateLimiter;
	address: RateLimiter;
	all: RateLimiter;
};

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
			limits: ConfirmLimits;
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

	/** `did`'s setting for `channel` as the settings page shows it, or null when it's off. */
	async view(did: string, channel: NotificationChannel): Promise<ChannelView | null> {
		const row = await this.get(did, channel);

		if (!row) return null;
		const topics = new Set(parseJson<string[]>(row.topics, []));

		return {
			status: row.status,
			pausedReason: row.pausedReason,
			address: row.address,
			topics: TOPICS.filter((topic) => topics.has(topic))
		};
	}

	/** Sends `channel` only `topics` from now on. */
	async setTopics(did: string, channel: NotificationChannel, topics: NotificationTopic[]): Promise<void> {
		await this.deps.db
			.updateTable('notificationChannel')
			.set({ topics: JSON.stringify(TOPICS.filter((topic) => topics.includes(topic))), updatedAt: nowIso() })
			.where('did', '=', did)
			.where('channel', '=', channel)
			.execute();
	}

	/**
	 * Reads the account's email address on sign-in when the grant allows it, for someone turning email on (`optIn`) or
	 * who has it on, so a changed or newly confirmed address is picked up. Says 'declined' when they were turning it on
	 * and didn't allow it, and 'limited' when the address needed a confirmation email the site wouldn't send now. A host
	 * that won't say leaves email as it was rather than failing the sign-in.
	 */
	async onSignIn(input: {
		did: string;
		scope: string;
		optIn: boolean;
		session: () => Promise<SessionEmail>;
	}): Promise<'declined' | 'read' | 'limited' | 'skipped'> {
		const { did, optIn } = input;

		if (!grantsEmail(input.scope, did)) return optIn ? 'declined' : 'skipped';

		if (!optIn && !(await this.get(did, 'email'))) return 'skipped';

		try {
			return (await this.readEmail(did, await input.session(), optIn)) ? 'read' : 'limited';
		} catch (error) {
			this.deps.logger.warn({ err: error, did }, 'could not read the email address');

			return 'skipped';
		}
	}

	/**
	 * Keeps the email address a sign-in with the permission to read it found, for someone who turned email on
	 * (`optIn`) or had it on already. The site only sends to an address it confirmed itself, since a host can call any
	 * address confirmed: a new or changed one waits on the link in one confirmation email, which goes again when they
	 * turn email on again. An address their host hasn't confirmed either waits on that first. Returns false when the
	 * address needed a confirmation email the limits held back.
	 */
	private async readEmail(did: string, session: SessionEmail, optIn: boolean): Promise<boolean> {
		const { db } = this.deps;
		const existing = await this.get(did, 'email');

		if (!existing && !optIn) return true;
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

		const token = sendLink && address && this.mayConfirm(did, address) ? randomBytes(32).toString('base64url') : null;

		// A link out for this address keeps working until it's replaced or expires; one for another address never does.
		const keepLink = pausedReason === 'email_unverified' && existing?.address === address;

		const values = {
			status: pausedReason ? ('paused' as const) : ('active' as const),
			pausedReason,
			address,
			addressReadAt: now,
			updatedAt: now,
			...(token && address
				? {
						confirmTokenHash: hashToken(token),
						confirmAddressHash: hashAddress(address),
						confirmExpiresAt: new Date(Date.now() + CONFIRM_MS).toISOString()
					}
				: keepLink
					? {}
					: { confirmTokenHash: null, confirmAddressHash: null, confirmExpiresAt: null })
		};

		await db
			.insertInto('notificationChannel')
			.values({
				did,
				channel: 'email',
				confirmTokenHash: null,
				confirmAddressHash: null,
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

		return !sendLink || Boolean(token);
	}

	/** Whether a confirmation email can go to `address` for `did` now. Asking past the limits leaves the last link working. */
	private mayConfirm(did: string, address: string): boolean {
		const { sendConfirmation, limits, logger } = this.deps;

		if (!sendConfirmation) return false;
		const addressKey = hashAddress(address);

		const allowed =
			limits.account.take(`confirm:${did}`) &&
			limits.accountAddress.take(`confirm:${addressKey}:${did}`) &&
			limits.address.take(`confirm-addr:${addressKey}`) &&
			limits.all.take('confirm');

		if (!allowed) logger.warn({ did }, 'not sending an email confirmation');

		return allowed;
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
				confirmAddressHash: null,
				confirmExpiresAt: null,
				updatedAt: nowIso()
			})
			.where('did', '=', row.did)
			.where('channel', '=', 'email')
			.where('confirmTokenHash', '=', hashToken(token))
			// Only while the address is still the one the link went to.
			.where('address', '=', row.address)
			.where('confirmAddressHash', '=', hashAddress(row.address))
			.execute();
		this.deps.logger.info({ did: row.did }, 'confirmed an email address');

		return 'confirmed';
	}

	private async byConfirmToken(
		token: string
	): Promise<{ state: 'invalid' } | { state: 'valid' | 'expired'; row: { did: string; address: string } }> {
		const row = token
			? await this.deps.db
					.selectFrom('notificationChannel')
					.select(['did', 'address', 'pausedReason', 'confirmAddressHash', 'confirmExpiresAt'])
					.where('channel', '=', 'email')
					.where('confirmTokenHash', '=', hashToken(token))
					.executeTakeFirst()
			: undefined;

		const forThisAddress = row?.address && row.confirmAddressHash === hashAddress(row.address);

		if (!row?.address || !forThisAddress || row.pausedReason !== 'email_unverified') return { state: 'invalid' };

		const state = !row.confirmExpiresAt || row.confirmExpiresAt < nowIso() ? 'expired' : 'valid';

		return { row: { did: row.did, address: row.address }, state };
	}

	/**
	 * Turns DMs on for `did`, or holds them while Bluesky chat won't deliver the root's DMs to them (`canDm` false), so
	 * the settings page can ask them to follow the root. Their DID is the address.
	 */
	async enableDm(did: string, canDm: boolean): Promise<void> {
		const now = nowIso();

		const values = {
			status: canDm ? ('active' as const) : ('paused' as const),
			pausedReason: canDm ? null : ('dm_not_allowed' as const),
			updatedAt: now
		};

		await this.deps.db
			.insertInto('notificationChannel')
			.values({
				did,
				channel: 'dm',
				...values,
				address: null,
				addressReadAt: null,
				unsubscribeToken: null,
				topics: JSON.stringify(TOPICS),
				lastSentAt: null,
				createdAt: now,
				confirmTokenHash: null,
				confirmAddressHash: null,
				confirmExpiresAt: null
			})
			.onConflict((oc) => oc.columns(['did', 'channel']).doUpdateSet(values))
			.execute();
		this.deps.logger.info({ did, status: values.status }, 'set up Bluesky DMs for the inbox');
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
