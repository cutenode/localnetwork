import { AppBskyRichtextFacet, AtpAgent } from '@atproto/api';
import { describe, expect, it } from 'vitest';
import type { NotificationView } from '#lib/types.ts';
import { silentLogger } from '../logger';
import { type DmMessage, dmChannel, LogDmSender, renderDm, RootDmSender } from './dm';

const neon = 'did:plc:neon';

const item = (title: string, overrides: Partial<NotificationView> = {}): NotificationView => ({
	id: title,
	topic: 'bookings',
	title,
	body: 'Warehouse night',
	href: '/dashboard/bookings/3mabc',
	occurredAt: '2026-10-28T21:15:00Z',
	read: false,
	...overrides
});

type Answer = Response | Error;

/**
 * The root's PDS, answering each XRPC method from `answers` (its next answer each call), and the requests it got. The
 * chat calls go through it with the proxy header, as they do for real.
 */
const network = (answers: Record<string, Answer[]>) => {
	const requests: { method: string; proxy: string | null; body: unknown }[] = [];

	const agent = new AtpAgent({
		service: 'https://pds.test',
		fetch: async (input, init) => {
			const request = new Request(input, init);
			const method = new URL(request.url).pathname.replace('/xrpc/', '');
			requests.push({
				method,
				proxy: request.headers.get('atproto-proxy'),
				body: request.body ? await request.json() : null
			});
			const answer = answers[method]?.shift() ?? Response.json({ error: 'MethodNotImplemented' }, { status: 501 });

			if (answer instanceof Error) throw answer;

			return answer;
		}
	});

	const sender = new RootDmSender({
		chat: () => agent.withProxy('bsky_chat', 'did:web:api.bsky.chat'),
		logger: silentLogger
	});

	return { sender, requests };
};

/** A conversation and a sent message as chat describes them. */
const convo = () =>
	Response.json({
		convo: {
			id: 'convo-1',
			rev: '1',
			members: [
				{ did: 'did:plc:root', handle: 'localnetwork.test' },
				{ did: neon, handle: 'neon.test' }
			],
			muted: false,
			unreadCount: 0
		}
	});

const sentMessage = (text: string) =>
	Response.json({ id: 'msg-1', rev: '2', text, sender: { did: 'did:plc:root' }, sentAt: '2026-10-29T12:00:00.000Z' });

/** An error chat answers with, by its lexicon name. */
const named = (status: number, error: string) => Response.json({ error, message: error }, { status });

const refused = (status: number) =>
	Response.json({ error: 'InvalidRequest', message: 'recipient has disabled incoming messages' }, { status });

describe('RootDmSender', () => {
	it('asks Bluesky chat alone whether the root can DM someone, reading nothing from the AppView', async () => {
		const { sender, requests } = network({
			'chat.bsky.convo.getConvoAvailability': [
				Response.json({ canChat: true }),
				Response.json({ canChat: false }),
				Response.json({ error: 'InternalServerError' }, { status: 500 })
			]
		});

		expect(await sender.availability(neon)).toBe('ok');
		expect(await sender.availability(neon)).toBe('not_allowed');
		expect(await sender.availability(neon)).toBe('unavailable');
		expect(requests.map((request) => [request.method, request.proxy])).toEqual(
			Array.from({ length: 3 }, () => ['chat.bsky.convo.getConvoAvailability', 'did:web:api.bsky.chat#bsky_chat'])
		);
	});

	it('sends into the conversation with them, with the links the message made', async () => {
		const { sender, requests } = network({
			'chat.bsky.convo.getConvoForMembers': [convo()],
			'chat.bsky.convo.sendMessage': [sentMessage('hi')]
		});

		const dm = renderDm([item('Hi @neon.test')], 'https://localnetwork.computer');
		expect(await sender.send(neon, dm)).toEqual({ kind: 'sent', providerId: 'msg-1' });

		const send = requests.find((request) => request.method === 'chat.bsky.convo.sendMessage');
		expect(send?.proxy).toBe('did:web:api.bsky.chat#bsky_chat');
		expect(send?.body).toMatchObject({
			convoId: 'convo-1',
			message: { text: dm.text, facets: dm.facets }
		});
	});

	it("tries again when chat is busy or unreachable before sending, and won't risk a second copy after", async () => {
		const hi = renderDm([item('Hi')], 'https://localnetwork.computer');

		const { sender } = network({
			'chat.bsky.convo.getConvoForMembers': [
				new TypeError('fetch failed'),
				refused(429),
				convo(),
				convo(),
				convo(),
				convo()
			],
			'chat.bsky.convo.sendMessage': [
				new TypeError('socket hang up'),
				named(400, 'ConvoLocked'),
				refused(401),
				refused(503)
			]
		});

		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'retryable' });
		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'retryable' });
		expect(await sender.send(neon, hi)).toMatchObject({
			kind: 'failed',
			error: expect.stringContaining('socket hang up')
		});
		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'rejected', reason: 'dm_not_allowed' });
		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'failed' });
		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'retryable' });
	});

	it('pauses DMs only when chat says the recipient refused them, and fails anything else without pausing', async () => {
		const hi = renderDm([item('Hi')], 'https://localnetwork.computer');
		const refusals = ['BlockedActor', 'BlockedSubject', 'MessagesDisabled', 'NotFollowedBySender', 'RecipientNotFound'];

		const { sender } = network({
			'chat.bsky.convo.getConvoForMembers': [
				...refusals.map((name) => named(400, name)),
				named(400, 'AccountSuspended'),
				convo(),
				convo()
			],
			'chat.bsky.convo.sendMessage': [named(400, 'InvalidRequest'), refused(403)]
		});

		for (const _ of refusals)
			expect(await sender.send(neon, hi)).toMatchObject({ kind: 'rejected', reason: 'dm_not_allowed' });
		// The root's own account, a message chat won't take, or a refusal it doesn't name: not the recipient's doing.
		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'failed' });
		expect(await sender.send(neon, hi)).toMatchObject({
			kind: 'failed',
			error: expect.stringContaining('InvalidRequest')
		});
		expect(await sender.send(neon, hi)).toMatchObject({ kind: 'failed' });
	});
});

describe('renderDm', () => {
	const url = 'https://localnetwork.computer';
	const bytes = (text: string) => new TextEncoder().encode(text);

	/** The text each link facet covers, read back from its byte offsets, and where it goes. */
	const links = ({ text, facets }: DmMessage) =>
		facets.map(({ index, features: [feature] }) => ({
			covers: new TextDecoder().decode(bytes(text).slice(index.byteStart, index.byteEnd)),
			uri: AppBskyRichtextFacet.isLink(feature) ? feature.uri : null
		}));

	it('words one item with its link, and several as a list pointing at the inbox', () => {
		const one = renderDm([item('Neon Tide sent a message')], url);

		expect(one.text).toBe(
			[
				'Neon Tide sent a message: Warehouse night',
				`${url}/dashboard/bookings/3mabc`,
				`Change what you get: ${url}/dashboard/inbox/settings`
			].join('\n\n')
		);

		const several = renderDm([item('One'), item('Two', { body: undefined })], url);
		expect(several.text).toContain('2 new things in your Local Network inbox:\n\n• One: Warehouse night\n• Two');
		expect(links(several)).toEqual([
			{ covers: `${url}/dashboard/inbox`, uri: `${url}/dashboard/inbox` },
			{ covers: `${url}/dashboard/inbox/settings`, uri: `${url}/dashboard/inbox/settings` }
		]);
	});

	it('links only its own pages, and breaks up addresses in what people wrote so no app links them', () => {
		const message = renderDm(
			[item('New booking inquiry for https://evil.example', { body: 'Party at evil.example/win' })],
			url
		);

		expect(links(message)).toEqual([
			{ covers: `${url}/dashboard/bookings/3mabc`, uri: `${url}/dashboard/bookings/3mabc` },
			{ covers: `${url}/dashboard/inbox/settings`, uri: `${url}/dashboard/inbox/settings` }
		]);
		expect(message.text).not.toContain('https://evil.example');
		expect(message.text).not.toContain('evil.example');
		expect(message.text).toContain('evil[.]example');
	});

	it("stays within chat's limits of 1,000 graphemes and 10,000 bytes, saying how many more there are", () => {
		// Each of these is one grapheme of 25 bytes.
		const family = '👩‍👩‍👧‍👦';
		const long = item(family.repeat(2000), { body: 'x'.repeat(5000) });

		const one = renderDm([long], url);

		const many = renderDm(
			Array.from({ length: 30 }, (_, n) => item(`Message ${n} ${'y'.repeat(80)}`)),
			url
		);

		for (const { text } of [one, many]) {
			expect([...new Intl.Segmenter().segment(text)].length).toBeLessThanOrEqual(1000);
			expect(bytes(text).length).toBeLessThanOrEqual(10_000);
		}

		expect(one.text).toContain('…');
		expect(many.text).toMatch(/\nand \d+ more\n/);
		expect(links(many).map((link) => link.uri)).toEqual([`${url}/dashboard/inbox`, `${url}/dashboard/inbox/settings`]);
	});
});

describe('dmChannel', () => {
	it('sends to the DID, a minute after the first item, or to the log in development', async () => {
		const log = new LogDmSender(silentLogger);
		const channel = dmChannel(log, 'http://127.0.0.1:5173');

		expect(channel.windowMs).toBe(60_000);
		expect(await log.availability()).toBe('ok');
		expect(await channel.send({ did: neon, address: null, unsubscribeToken: null }, [item('Hi')], 'k')).toEqual({
			kind: 'sent',
			providerId: null
		});
	});
});
