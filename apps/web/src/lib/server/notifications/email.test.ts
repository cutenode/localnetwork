import { describe, expect, it } from 'vitest';
import type { NotificationView } from '#lib/types.ts';
import { silentLogger } from '../logger';
import { confirmationSender, type Email, emailChannel, LogEmailSender, renderEmail, ResendEmailSender } from './email';

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

const to = { did: 'did:plc:neon', address: 'neon@example.test', unsubscribeToken: 'tok en' };

const email = renderEmail([item('Neon Tide sent a message')], to, 'https://localnetwork.computer');

/** Resend, answering every request with `respond`, and the requests it got. */
const resend = (respond: () => Response | Promise<Response>) => {
	const requests: Request[] = [];

	const sender = new ResendEmailSender({
		apiKey: 're_test',
		from: 'Local Network <inbox@localnetwork.computer>',
		fetch: async (input, init) => {
			requests.push(new Request(input, init));

			return respond();
		}
	});

	return { sender, requests };
};

describe('renderEmail', () => {
	it('words one item as its own subject, and several as a count, each linking to the site', () => {
		expect(email.subject).toBe('Neon Tide sent a message');
		expect(email.text).toContain(
			'Neon Tide sent a message: Warehouse night\nhttps://localnetwork.computer/dashboard/bookings/3mabc'
		);

		const several = renderEmail([item('One'), item('<Two>', { body: undefined })], to, 'https://localnetwork.computer');
		expect(several.subject).toBe('2 new things in your Local Network inbox');
		expect(several.html).toContain('&lt;Two&gt;</a>');
		expect(several.html).not.toContain('<Two>');
	});

	it('offers a link that turns email off, in the body and where mail apps look for it', () => {
		const unsubscribe = 'https://localnetwork.computer/notifications/unsubscribe?t=tok%20en';

		expect(email.headers).toEqual({ 'List-Unsubscribe': `<${unsubscribe}>` });
		expect(email.text).toContain(`Turn these emails off: ${unsubscribe}`);
		expect(email.to).toBe('neon@example.test');
	});
});

describe('ResendEmailSender', () => {
	it('sends with its key and the idempotency key, and keeps the id Resend gives', async () => {
		const { sender, requests } = resend(() => Response.json({ id: 'msg_1' }));

		expect(await sender.send(email, 'batch-1')).toEqual({ kind: 'sent', providerId: 'msg_1' });
		const [request] = requests;
		expect(request.url).toBe('https://api.resend.com/emails');
		expect(request.headers.get('authorization')).toBe('Bearer re_test');
		expect(request.headers.get('idempotency-key')).toBe('batch-1');
		expect(await request.json()).toEqual({
			from: 'Local Network <inbox@localnetwork.computer>',
			to: ['neon@example.test'],
			subject: email.subject,
			text: email.text,
			html: email.html,
			headers: email.headers
		});
	});

	it("tries again when Resend is busy, down, or doesn't answer", async () => {
		const busy = resend(() => new Response('slow down', { status: 429, headers: { 'retry-after': '20' } }));
		expect(await busy.sender.send(email, 'k')).toMatchObject({ kind: 'retryable', retryAfterMs: 20_000 });

		const down = resend(() => new Response('', { status: 503 }));
		expect(await down.sender.send(email, 'k')).toMatchObject({ kind: 'retryable', retryAfterMs: undefined });

		const offline = resend(() => Promise.reject(new TypeError('fetch failed')));
		expect(await offline.sender.send(email, 'k')).toMatchObject({
			kind: 'retryable',
			error: expect.stringContaining('fetch failed')
		});
	});

	it('pauses an address Resend refuses, and fails without blaming it when the site is set up wrong', async () => {
		const invalid = resend(() => Response.json({ name: 'validation_error' }, { status: 422 }));
		expect(await invalid.sender.send(email, 'k')).toMatchObject({ kind: 'rejected', reason: 'address_rejected' });

		const unverified = resend(() => Response.json({ name: 'invalid_from_address' }, { status: 403 }));
		expect(await unverified.sender.send(email, 'k')).toMatchObject({
			kind: 'failed',
			error: expect.stringContaining('403')
		});

		const odd = resend(() => new Response('not json'));
		expect(await odd.sender.send(email, 'k')).toEqual({ kind: 'sent', providerId: null });
	});
});

describe('emailChannel', () => {
	it('sends through its sender, or to the log in development, and refuses a channel with no address', async () => {
		const channel = emailChannel(new LogEmailSender(silentLogger), 'http://127.0.0.1:5173');

		expect(channel.windowMs).toBe(120_000);
		expect(await channel.send(to, [item('Hi')], 'k')).toEqual({ kind: 'sent', providerId: null });
		expect(await channel.send({ ...to, address: null }, [item('Hi')], 'k')).toMatchObject({
			kind: 'rejected',
			reason: 'email_unconfirmed'
		});
	});
});

describe('confirmationSender', () => {
	it('emails just the link that confirms the address, once per link, and only logs a failure', async () => {
		const sent: { email: Email; key: string }[] = [];

		const confirm = confirmationSender(
			{
				send: async (email, key) => {
					sent.push({ email, key });

					return sent.length > 1 ? { kind: 'failed', error: 'down' } : { kind: 'sent', providerId: null };
				}
			},
			'https://localnetwork.computer',
			silentLogger
		);

		await confirm('neon@example.test', 'a+b');
		await confirm('neon@example.test', 'c');

		const [{ email, key }] = sent;
		expect(email).toMatchObject({
			to: 'neon@example.test',
			subject: 'Confirm your email for Local Network',
			headers: {}
		});
		expect(email.text).toContain('https://localnetwork.computer/notifications/confirm?t=a%2Bb');
		expect(email.text).not.toContain('unsubscribe');
		expect(key).toMatch(/^confirm-[0-9a-f]{64}$/);
		expect(sent[1].key).not.toBe(key);
	});
});
