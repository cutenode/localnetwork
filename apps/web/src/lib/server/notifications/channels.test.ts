import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { openDatabase } from '../db';
import { silentLogger } from '../logger';
import { RateLimiter } from '../rate-limit';
import { type ConfirmLimits, NotificationChannels, type SessionEmail } from './channels';

const neon = 'did:plc:neon';

const withEmail = 'atproto account:email?action=read';

const confirmed = (email: string): SessionEmail => ({ email, emailConfirmed: true });

/** The site's limits on confirmation emails: per account, per address, and overall. */
const confirmLimits = (overrides: Partial<ConfirmLimits> = {}): ConfirmLimits => ({
	account: new RateLimiter(5, 5 / 86_400),
	accountAddress: new RateLimiter(3, 3 / 86_400),
	address: new RateLimiter(10, 10 / 86_400),
	all: new RateLimiter(100, 100 / 3600),
	...overrides
});

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
			limits: confirmLimits()
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

	it('lets a link work for a day, and sends a new one for a changed address or when asked again, a few a day', async () => {
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
		// Three to that address today, so the third ask sends nothing.
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

	it('only lets a link confirm the address it went to', async () => {
		channels = new NotificationChannels({
			db: await openDatabase(':memory:'),
			logger: silentLogger,
			sendConfirmation: async (address, token) => {
				links.push({ address, token });
			},
			limits: confirmLimits({ all: new RateLimiter(1, 1 / 3600) })
		});

		await signIn(confirmed('neon@example.test'), { optIn: true });
		// The address changes while the link is out, and the limit holds back a new one.
		expect(await signIn(confirmed('neon@new.example'))).toBe('limited');
		expect(links).toHaveLength(1);

		expect(await channels.confirmation(links[0].token)).toBe('invalid');
		expect(await channels.confirmEmail(links[0].token)).toBe('invalid');
		expect(await email()).toMatchObject({
			status: 'paused',
			pausedReason: 'email_unverified',
			address: 'neon@new.example'
		});
	});

	it('lets nobody hold up the real owner of an address, and only confirms the account each link went to', async () => {
		const db = await openDatabase(':memory:');
		const sent = new Map<string, string>();

		const as = (did: string) =>
			new NotificationChannels({
				db,
				logger: silentLogger,
				sendConfirmation: async (_address, token) => {
					sent.set(did, token);
				},
				limits: confirmLimits()
			});

		const ask = (did: string) =>
			as(did).onSignIn({ did, scope: withEmail, optIn: true, session: async () => confirmed('Owner@Example.test') });

		// Someone else asked first, for an address that isn't theirs.
		expect(await ask('did:plc:squatter')).toBe('read');
		expect(await ask('did:plc:owner')).toBe('read');

		const owner = as('did:plc:owner');
		expect(await owner.confirmEmail(sent.get('did:plc:owner') ?? '')).toBe('confirmed');
		expect((await owner.get('did:plc:owner', 'email'))?.status).toBe('active');
		expect((await owner.get('did:plc:squatter', 'email'))?.status).toBe('paused');
	});

	it('emails one address a few links a day per account, ten across all of them, and the site a hundred an hour', async () => {
		const db = await openDatabase(':memory:');
		const limits = confirmLimits({ all: new RateLimiter(14, 14 / 3600) });
		let count = 0;

		const ask = (did: string, address = 'victim@example.test') =>
			new NotificationChannels({
				db,
				logger: silentLogger,
				sendConfirmation: async () => {
					count += 1;
				},
				limits
			}).onSignIn({ did, scope: withEmail, optIn: true, session: async () => confirmed(address) });

		for (const _ of [1, 2, 3]) expect(await ask('did:plc:one')).toBe('read');
		expect(await ask('did:plc:one')).toBe('limited');

		for (const n of [2, 3, 4, 5, 6, 7, 8]) expect(await ask(`did:plc:other${n}`)).toBe('read');

		// Ten to the address today, from any accounts.
		expect(await ask('did:plc:other9')).toBe('limited');
		expect(count).toBe(10);

		for (const n of [1, 2, 3, 4]) expect(await ask(`did:plc:else${n}`, `else${n}@example.test`)).toBe('read');
		// Fourteen overall in the hour.
		expect(await ask('did:plc:else5', 'else5@example.test')).toBe('limited');
		expect(count).toBe(14);
	});

	it('stops one account that keeps changing its address at five confirmation emails a day', async () => {
		let count = 0;

		const rotating = new NotificationChannels({
			db: await openDatabase(':memory:'),
			logger: silentLogger,
			sendConfirmation: async () => {
				count += 1;
			},
			limits: confirmLimits()
		});

		const ask = (n: number) =>
			rotating.onSignIn({
				did: neon,
				scope: withEmail,
				optIn: true,
				session: async () => confirmed(`victim${n}@example.test`)
			});

		for (const n of [1, 2, 3, 4, 5]) expect(await ask(n)).toBe('read');
		expect(await ask(6)).toBe('limited');
		expect(count).toBe(5);
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

	it('shows the setting for the settings page, and narrows what it sends to the topics picked', async () => {
		expect(await channels.view(neon, 'email')).toBeNull();
		await signIn(confirmed('neon@example.test'), { optIn: true });
		await channels.confirmEmail(links[0].token);

		await channels.setTopics(neon, 'email', ['events', 'bookings']);
		expect(await channels.view(neon, 'email')).toEqual({
			status: 'active',
			pausedReason: null,
			address: 'neon@example.test',
			topics: ['bookings', 'events']
		});

		await channels.setTopics(neon, 'email', []);
		expect((await channels.view(neon, 'email'))?.topics).toEqual([]);
		await channels.disable(neon, 'email');
		expect(await channels.view(neon, 'email')).toBeNull();
	});

	it('turns DMs on, or holds them until Bluesky chat will deliver them, with no address to keep', async () => {
		await channels.enableDm(neon, false);
		expect(await channels.view(neon, 'dm')).toEqual({
			status: 'paused',
			pausedReason: 'dm_not_allowed',
			address: null,
			topics: ['bookings', 'guests', 'waitlist', 'events']
		});

		await channels.setTopics(neon, 'dm', ['bookings']);
		await channels.enableDm(neon, true);
		expect(await channels.view(neon, 'dm')).toMatchObject({
			status: 'active',
			pausedReason: null,
			topics: ['bookings']
		});
		expect(await channels.view(neon, 'email')).toBeNull();
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
