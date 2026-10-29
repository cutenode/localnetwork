import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { openDatabase } from '../db';
import { silentLogger } from '../logger';
import { RateLimiter } from '../rate-limit';
import { NotificationChannels, type SessionEmail } from './channels';

const neon = 'did:plc:neon';

const withEmail = 'atproto account:email?action=read';

const confirmed = (email: string): SessionEmail => ({ email, emailConfirmed: true });

describe('NotificationChannels', () => {
	let channels: NotificationChannels;
	/** Each confirmation email: where it went and the link's token. */
	let links: { address: string; token: string }[];

	const signIn = (session: SessionEmail | Error, options: { optIn?: boolean; scope?: string } = {}) =>
		channels.onSignIn({
			did: neon,
			scope: options.scope ?? withEmail,
			optIn: options.optIn ?? false,
			session: async () => {
				if (session instanceof Error) throw session;

				return session;
			}
		});

	const email = () => channels.get(neon, 'email');

	beforeEach(async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		links = [];

		channels = new NotificationChannels({
			db: await openDatabase(':memory:'),
			logger: silentLogger,
			sendConfirmation: async (address, token) => {
				links.push({ address, token });
			},
			confirmLimit: new RateLimiter(3, 3 / 3600)
		});
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it("never takes the host's word for an address: it stays paused until the link emailed to it is opened", async () => {
		expect(await signIn(confirmed('victim@example.test'), { optIn: true })).toBe('read');
		expect(await email()).toMatchObject({
			status: 'paused',
			pausedReason: 'email_unverified',
			address: 'victim@example.test'
		});

		const [{ address, token }] = links;
		expect(address).toBe('victim@example.test');
		// Only a hash of the link is kept.
		expect(JSON.stringify(await email())).not.toContain(token);
		expect(await channels.confirmation(token)).toBe('valid');
		expect(await channels.confirmation('made-up')).toBe('invalid');
		expect(await channels.confirmation('')).toBe('invalid');

		expect(await channels.confirmEmail(token)).toBe('confirmed');
		expect(await email()).toMatchObject({
			status: 'active',
			pausedReason: null,
			topics: '["bookings","guests","waitlist","events"]',
			unsubscribeToken: expect.stringMatching(/^[\w-]{43}$/),
			confirmTokenHash: null
		});
		expect(await channels.confirmEmail(token)).toBe('invalid');
	});

	it('lets a link work for a day, and sends a new one for a changed address or when asked again, a few an hour', async () => {
		await signIn(confirmed('neon@example.test'), { optIn: true });
		vi.setSystemTime(Date.now() + 25 * 3_600_000);
		expect(await channels.confirmEmail(links[0].token)).toBe('expired');

		// Signing in again with the same address sends nothing more.
		await signIn(confirmed('neon@example.test'));
		expect(links).toHaveLength(1);

		await signIn(confirmed('neon@new.example'));
		expect(links.map((link) => link.address)).toEqual(['neon@example.test', 'neon@new.example']);
		expect(await channels.confirmation(links[0].token)).toBe('invalid');

		for (const _ of [1, 2, 3]) await signIn(confirmed('neon@new.example'), { optIn: true });
		// Three in the hour, so the third ask sends nothing.
		expect(links).toHaveLength(4);
		expect(await email()).toMatchObject({ status: 'paused', pausedReason: 'email_unverified' });

		// The ask that sent nothing left the last link working.
		expect(await channels.confirmEmail(links[3].token)).toBe('confirmed');
		// Once confirmed here, a changed address goes back to waiting on a link.
		await signIn(confirmed('neon@other.example'));
		expect(await email()).toMatchObject({
			status: 'paused',
			pausedReason: 'email_unverified',
			address: 'neon@other.example'
		});
	});

	it('waits for the host to confirm an address before emailing it anything, and keeps a refused one paused', async () => {
		await signIn({ email: 'neon@example.test', emailConfirmed: false }, { optIn: true });
		expect(await email()).toMatchObject({ status: 'paused', pausedReason: 'email_unconfirmed' });
		expect(links).toEqual([]);
		const { unsubscribeToken } = (await email())!;

		await signIn(confirmed('neon@example.test'));
		await channels.confirmEmail(links[0].token);
		expect(await email()).toMatchObject({ status: 'active', unsubscribeToken });

		// An address the email service refused stays paused until it changes, or they turn email on again.
		await channels.pause(neon, 'email', 'address_rejected');
		await signIn(confirmed('neon@example.test'));
		expect((await email())?.pausedReason).toBe('address_rejected');
		await signIn(confirmed('neon@example.test'), { optIn: true });
		expect((await email())?.status).toBe('active');
		expect(links).toHaveLength(1);
	});

	it('reads nothing for someone who has email off, says when they declined, and never fails the sign-in', async () => {
		expect(await signIn(confirmed('neon@example.test'))).toBe('skipped');
		expect(await signIn({ email: 'neon@example.test' }, { optIn: true, scope: 'atproto' })).toBe('declined');
		expect(await signIn({ email: 'neon@example.test' }, { scope: 'atproto' })).toBe('skipped');
		expect(await signIn(new Error('host down'), { optIn: true })).toBe('skipped');
		expect(await email()).toBeUndefined();

		// A host that shares no address at all leaves email waiting on one.
		await signIn({}, { optIn: true });
		expect(await email()).toMatchObject({ status: 'paused', address: null });
	});

	it('turns email off from its unsubscribe link once, forgetting the address', async () => {
		await signIn(confirmed('neon@example.test'), { optIn: true });
		const token = (await email())?.unsubscribeToken ?? '';

		expect(await channels.hasToken(token)).toBe(true);
		expect(await channels.hasToken('made-up')).toBe(false);
		expect(await channels.unsubscribe(token)).toBe(true);
		expect(await email()).toBeUndefined();
		expect(await channels.unsubscribe(token)).toBe(false);
		expect(await channels.hasToken(token)).toBe(false);
	});
});
