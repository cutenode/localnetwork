import { type Agent, type AppBskyRichtextFacet, XRPCError } from '@atproto/api';
import type { DmAvailability, NotificationView } from '#lib/types.ts';
import type { Logger } from '../logger';
import type { ChannelSender, SendOutcome } from './outbox';

/** A DM's text, and the links in it, which are only ever the site's own pages. */
export type DmMessage = { text: string; facets: AppBskyRichtextFacet.Main[] };

/** Sends Bluesky DMs from the root account. */
export type DmSender = {
	/** Whether the root can DM `did` now, and if not, why. */
	availability(did: string): Promise<DmAvailability>;
	send(did: string, message: DmMessage): Promise<SendOutcome>;
};

/**
 * What chat.bsky.convo.defs#messageInput allows in `text` (@atproto/api's lexicons.js, `maxLength: 10000` bytes and
 * `maxGraphemes: 1000`).
 */
const MAX_BYTES = 10_000;

const MAX_GRAPHEMES = 1000;

/** The most of one item a DM shows, so one long title can't crowd out the rest. */
const LINE_GRAPHEMES = 200;

/**
 * The errors chat.bsky.convo.getConvoForMembers and sendMessage declare (in @atproto/api's lexicons.js) that mean the
 * recipient won't take DMs from the root: blocks either way, DMs off or limited to accounts they follow, an account
 * that's gone, or a conversation locked against new messages. Anything else, like a message chat won't take, or
 * AccountSuspended (the root's own), isn't theirs to fix, so it doesn't pause their DMs.
 */
const REFUSED = new Set([
	'BlockedActor',
	'BlockedSubject',
	'MessagesDisabled',
	'NotFollowedBySender',
	'RecipientNotFound',
	'ConvoLocked'
]);

/** What a chat call's failure means for the message: try again later, the recipient won't take it, or neither. */
function outcomeOf(error: Error, sent: boolean): SendOutcome {
	const message = `Bluesky chat: ${error.message}`;

	// The client reports a request that got no HTTP answer with a status below 100. Its statuses are an enum of the
	// HTTP codes, compared here as the numbers they are.
	const status = error instanceof XRPCError ? Number(error.status) : 0;

	if (!(error instanceof XRPCError) || status < 100) {
		// A dropped connection after sending might have delivered it, and a DM has no idempotency key to retry under.
		return sent ? { kind: 'failed', error: message } : { kind: 'retryable', error: message };
	}

	if (status === 429 || status >= 500) return { kind: 'retryable', error: message };

	if (REFUSED.has(error.error)) return { kind: 'rejected', reason: 'dm_not_allowed', error: message };

	return { kind: 'failed', error: `${message} (${error.error})` };
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

	async send(did: string, message: DmMessage): Promise<SendOutcome> {
		const chat = this.deps.chat();
		let convoId: string;

		try {
			convoId = (await chat.chat.bsky.convo.getConvoForMembers({ members: [did] })).data.convo.id;
		} catch (error) {
			return outcomeOf(error instanceof Error ? error : new Error(String(error)), false);
		}

		try {
			const { data } = await chat.chat.bsky.convo.sendMessage({ convoId, message });

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

	async send(did: string, { text }: DmMessage): Promise<SendOutcome> {
		this.logger.info({ did, text }, 'DM not sent (DM_PROVIDER=log)');

		return { kind: 'sent', providerId: null };
	}
}

const graphemes = (text: string) => [...new Intl.Segmenter().segment(text)].map((part) => part.segment);

const byteLength = (text: string) => new TextEncoder().encode(text).length;

/** `text` cut to `max` graphemes, ending in an ellipsis when it was longer. */
function clip(text: string, max: number): string {
	const parts = graphemes(text);

	return parts.length <= max ? text : `${parts.slice(0, max - 1).join('')}…`;
}

/**
 * Breaks up web addresses in text someone else wrote (a show's title, an act's name), so no chat app links them from
 * the site's own account: `https://` loses its scheme separator and a dot before a letter, as in a domain, becomes `[.]`.
 */
const defang = (text: string) => text.replaceAll('://', '[:]//').replace(/(?<=[\p{L}\p{N}])\.(?=\p{L})/gu, '[.]');

/**
 * Builds a DM from paragraphs, each some text and then maybe one of the site's own URLs, which alone get linked, at
 * byte offsets counted here rather than detected.
 */
function compose(paragraphs: { text?: string; url?: string }[]): DmMessage {
	let text = '';
	const facets: AppBskyRichtextFacet.Main[] = [];

	for (const [index, paragraph] of paragraphs.entries()) {
		if (index > 0) text += '\n\n';

		if (paragraph.text) text += paragraph.url ? `${paragraph.text} ` : paragraph.text;

		if (!paragraph.url) continue;
		const byteStart = byteLength(text);
		text += paragraph.url;
		facets.push({
			index: { byteStart, byteEnd: byteLength(text) },
			features: [{ $type: 'app.bsky.richtext.facet#link', uri: paragraph.url }]
		});
	}

	return { text, facets };
}

const fits = (dm: DmMessage) => graphemes(dm.text).length <= MAX_GRAPHEMES && byteLength(dm.text) <= MAX_BYTES;

/**
 * A batch of inbox items as one DM, within chat's limits: each item cut to a couple of hundred graphemes, and as many
 * as fit, then how many more there are.
 */
export function renderDm(items: NotificationView[], publicUrl: string): DmMessage {
	const line = (item: NotificationView) =>
		clip(defang([item.title, item.body].filter(Boolean).join(': ')), LINE_GRAPHEMES);

	const settings = { text: 'Change what you get:', url: `${publicUrl}/dashboard/inbox/settings` };

	if (items.length === 1) return compose([{ text: line(items[0]) }, { url: `${publicUrl}${items[0].href}` }, settings]);

	const build = (shown: number) => {
		const more = items.length - shown;
		const list = items.slice(0, shown).map((item) => `• ${line(item)}`);

		return compose([
			{ text: `${items.length} new things in your Local Network inbox:` },
			{ text: [...list, ...(more > 0 ? [`and ${more} more`] : [])].join('\n') },
			{ url: `${publicUrl}/dashboard/inbox` },
			settings
		]);
	};

	let shown = items.length;

	while (shown > 1 && !fits(build(shown))) shown -= 1;

	return build(shown);
}

/** DMs as an outbox channel: items wait a minute for others, so a burst of replies is one DM. */
export function dmChannel(sender: DmSender, publicUrl: string): ChannelSender {
	return {
		windowMs: 60_000,
		send: (to, items) => sender.send(to.did, renderDm(items, publicUrl))
	};
}
