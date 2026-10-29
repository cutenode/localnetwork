import { AtpAgent } from '@atproto/api';
import { describe, expect, it } from 'vitest';
import type { NotificationView } from '#lib/types.ts';
import { silentLogger } from '../logger';
import { dmChannel, LogDmSender, renderDm, RootDmSender } from './dm';

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
		bluesky: () => agent,
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

const message = (text: string) =>
	Response.json({ id: 'msg-1', rev: '2', text, sender: { did: 'did:plc:root' }, sentAt: '2026-10-29T12:00:00.000Z' });

const profile = (allowIncoming: string) =>
	Response.json({ did: neon, handle: 'neon.test', associated: { chat: { allowIncoming } } });

const refused = (status: number) =>
	Response.json({ error: 'InvalidRequest', message: 'recipient has disabled incoming messages' }, { status });

describe('RootDmSender', () => {
	it('asks Bluesky chat whether the root can DM someone, and why not', async () => {
		const { sender, requests } = network({
			'chat.bsky.convo.getConvoAvailability': [
				Response.json({ canChat: true }),
				Response.json({ canChat: false }),
				Response.json({ canChat: false }),
				Response.json({ error: 'InternalServerError' }, { status: 500 })
			],
			'app.bsky.actor.getProfile': [profile('following'), profile('none')]
		});

		expect(await sender.availability(neon)).toBe('ok');
		expect(await sender.availability(neon)).toBe('needs_follow');
		expect(await sender.availability(neon)).toBe('closed');
		expect(await sender.availability(neon)).toBe('unavailable');
		expect(requests[0].proxy).toBe('did:web:api.bsky.chat#bsky_chat');
	});

	it('sends into the conversation with them, linking the links and nothing else', async () => {
		const { sender, requests } = network({
			'chat.bsky.convo.getConvoForMembers': [convo()],
			'chat.bsky.convo.sendMessage': [message('hi')]
		});

		const text = 'Hi @neon.test, see https://localnetwork.computer/dashboard/inbox';
		expect(await sender.send(neon, text)).toEqual({ kind: 'sent', providerId: 'msg-1' });

		const send = requests.find((request) => request.method === 'chat.bsky.convo.sendMessage');
		expect(send?.proxy).toBe('did:web:api.bsky.chat#bsky_chat');
		expect(send?.body).toMatchObject({
			convoId: 'convo-1',
			message: {
				text,
				facets: [
					{
						features: [{ $type: 'app.bsky.richtext.facet#link', uri: 'https://localnetwork.computer/dashboard/inbox' }]
					}
				]
			}
		});
	});

	it("tries again when chat is busy or unreachable before sending, and won't risk a second copy after", async () => {
		const { sender } = network({
			'chat.bsky.convo.getConvoForMembers': [
				new TypeError('fetch failed'),
				refused(429),
				convo(),
				convo(),
				convo(),
				convo()
			],
			'chat.bsky.convo.sendMessage': [new TypeError('socket hang up'), refused(400), refused(401), refused(503)]
		});

		expect(await sender.send(neon, 'hi')).toMatchObject({ kind: 'retryable' });
		expect(await sender.send(neon, 'hi')).toMatchObject({ kind: 'retryable' });
		expect(await sender.send(neon, 'hi')).toMatchObject({
			kind: 'failed',
			error: expect.stringContaining('socket hang up')
		});
		expect(await sender.send(neon, 'hi')).toMatchObject({ kind: 'rejected', reason: 'dm_not_allowed' });
		expect(await sender.send(neon, 'hi')).toMatchObject({ kind: 'failed' });
		expect(await sender.send(neon, 'hi')).toMatchObject({ kind: 'retryable' });
	});
});

describe('renderDm', () => {
	it('words one item with its link, and several as a list pointing at the inbox', () => {
		const url = 'https://localnetwork.computer';

		expect(renderDm([item('Neon Tide sent a message')], url)).toBe(
			[
				'Neon Tide sent a message: Warehouse night',
				`${url}/dashboard/bookings/3mabc`,
				`Change what you get: ${url}/dashboard/inbox/settings`
			].join('\n\n')
		);

		const several = renderDm([item('One'), item('Two', { body: undefined })], url);
		expect(several).toContain('2 new things in your Local Network inbox:\n\n• One: Warehouse night\n• Two');
		expect(several).toContain(`${url}/dashboard/inbox\n\n`);
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
