import { type Agent, RichText, XRPCError } from '@atproto/api';
import type { DmAvailability, NotificationView } from '#lib/types.ts';
import type { Logger } from '../logger';
import type { ChannelSender, SendOutcome } from './outbox';

/** Sends Bluesky DMs from the root account. */
export type DmSender = {
	/** Whether the root can DM `did` now, and if not, why. */
	availability(did: string): Promise<DmAvailability>;
	send(did: string, text: string): Promise<SendOutcome>;
};

const LINK = 'app.bsky.richtext.facet#link';

/** What a chat call's failure means for the message: try again later, the recipient won't take it, or neither. */
function outcomeOf(error: Error, sent: boolean): SendOutcome {
	const message = `Bluesky chat: ${error.message}`;

	// The client reports a request that got no HTTP answer with a status below 100.
	if (!(error instanceof XRPCError) || error.status < 100) {
		// A dropped connection after sending might have delivered it, and a DM has no idempotency key to retry under.
		return sent ? { kind: 'failed', error: message } : { kind: 'retryable', error: message };
	}

	if (error.status === 429 || error.status >= 500) return { kind: 'retryable', error: message };

	// Chat answers 400 when the recipient doesn't accept DMs from the root, and 403 when either side blocked the other.
	if (error.status === 400 || error.status === 403)
		return { kind: 'rejected', reason: 'dm_not_allowed', error: message };

	return { kind: 'failed', error: message };
}

/** Sends through Bluesky chat, as the root account. */
export class RootDmSender implements DmSender {
	constructor(
		private readonly deps: {
			/** The root's agent proxied to the chat service. */
			chat: () => Agent;
			/** The root's agent, for reading profiles from Bluesky's AppView. */
			bluesky: () => Agent;
			logger: Logger;
		}
	) {}

	async availability(did: string): Promise<DmAvailability> {
		try {
			const { data } = await this.deps.chat().chat.bsky.convo.getConvoAvailability({ members: [did] });

			if (data.canChat) return 'ok';
			const { data: profile } = await this.deps.bluesky().app.bsky.actor.getProfile({ actor: did });

			return profile.associated?.chat?.allowIncoming === 'following' ? 'needs_follow' : 'closed';
		} catch (error) {
			this.deps.logger.warn({ err: error, did }, 'could not check whether the root can DM someone');

			return 'unavailable';
		}
	}

	async send(did: string, text: string): Promise<SendOutcome> {
		const chat = this.deps.chat();
		let convoId: string;

		try {
			convoId = (await chat.chat.bsky.convo.getConvoForMembers({ members: [did] })).data.convo.id;
		} catch (error) {
			return outcomeOf(error instanceof Error ? error : new Error(String(error)), false);
		}

		const rich = new RichText({ text });
		rich.detectFacetsWithoutResolution();
		// Mentions found this way have no DID yet, so only links go along.
		const facets = rich.facets?.filter((facet) => facet.features.every((feature) => feature.$type === LINK));

		try {
			const { data } = await chat.chat.bsky.convo.sendMessage({ convoId, message: { text: rich.text, facets } });

			return { kind: 'sent', providerId: data.id };
		} catch (error) {
			return outcomeOf(error instanceof Error ? error : new Error(String(error)), true);
		}
	}
}

/** Writes each DM to the log instead, for development, which runs no chat service. */
export class LogDmSender implements DmSender {
	constructor(private readonly logger: Logger) {}

	async availability(): Promise<DmAvailability> {
		return 'ok';
	}

	async send(did: string, text: string): Promise<SendOutcome> {
		this.logger.info({ did, text }, 'DM not sent (DM_PROVIDER=log)');

		return { kind: 'sent', providerId: null };
	}
}

/** A batch of inbox items as one DM. */
export function renderDm(items: NotificationView[], publicUrl: string): string {
	const line = (item: NotificationView) => [item.title, item.body].filter(Boolean).join(': ');
	const settings = `Change what you get: ${publicUrl}/dashboard/inbox/settings`;

	if (items.length === 1) return [line(items[0]), `${publicUrl}${items[0].href}`, settings].join('\n\n');

	return [
		`${items.length} new things in your Local Network inbox:`,
		items.map((item) => `• ${line(item)}`).join('\n'),
		`${publicUrl}/dashboard/inbox`,
		settings
	].join('\n\n');
}

/** DMs as an outbox channel: items wait a minute for others, so a burst of replies is one DM. */
export function dmChannel(sender: DmSender, publicUrl: string): ChannelSender {
	return {
		windowMs: 60_000,
		send: (to, items) => sender.send(to.did, renderDm(items, publicUrl))
	};
}
