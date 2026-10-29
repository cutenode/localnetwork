import { beforeEach, describe, expect, it } from 'vitest';
import { openDatabase } from '../db';
import { silentLogger } from '../logger';
import { NotificationChannels, type SessionEmail } from './channels';

const neon = 'did:plc:neon';

const withEmail = 'atproto account:email?action=read';

describe('NotificationChannels', () => {
	let channels: NotificationChannels;

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

	beforeEach(async () => {
		channels = new NotificationChannels({ db: await openDatabase(':memory:'), logger: silentLogger });
	});

	it('keeps the address of someone turning email on, once their host has confirmed it, sending every topic', async () => {
		expect(await signIn({ email: 'neon@example.test', emailConfirmed: true }, { optIn: true })).toBe('read');

		expect(await channels.get(neon, 'email')).toMatchObject({
			status: 'active',
			pausedReason: null,
			address: 'neon@example.test',
			topics: '["bookings","guests","waitlist","events"]',
			unsubscribeToken: expect.stringMatching(/^[\w-]{43}$/)
		});
	});

	it('pauses email until the host confirms the address, and picks up a confirmed or changed one on a later sign-in', async () => {
		await signIn({ email: 'neon@example.test', emailConfirmed: false }, { optIn: true });
		expect(await channels.get(neon, 'email')).toMatchObject({ status: 'paused', pausedReason: 'email_unconfirmed' });
		const { unsubscribeToken } = (await channels.get(neon, 'email'))!;

		await signIn({ email: 'neon@example.test', emailConfirmed: true });
		expect(await channels.get(neon, 'email')).toMatchObject({ status: 'active', unsubscribeToken });

		// An address the email service refused stays paused until it changes, or they turn email on again.
		await channels.pause(neon, 'email', 'address_rejected');
		await signIn({ email: 'neon@example.test', emailConfirmed: true });
		expect((await channels.get(neon, 'email'))?.status).toBe('paused');
		await signIn({ email: 'neon@new.example', emailConfirmed: true });
		expect(await channels.get(neon, 'email')).toMatchObject({ status: 'active', address: 'neon@new.example' });
		await channels.pause(neon, 'email', 'address_rejected');
		await signIn({ email: 'neon@new.example', emailConfirmed: true }, { optIn: true });
		expect((await channels.get(neon, 'email'))?.status).toBe('active');
	});

	it('reads nothing for someone who has email off, says when they declined, and never fails the sign-in', async () => {
		expect(await signIn({ email: 'neon@example.test', emailConfirmed: true })).toBe('skipped');
		expect(await signIn({ email: 'neon@example.test' }, { optIn: true, scope: 'atproto' })).toBe('declined');
		expect(await signIn({ email: 'neon@example.test' }, { scope: 'atproto' })).toBe('skipped');
		expect(await signIn(new Error('host down'), { optIn: true })).toBe('skipped');
		expect(await channels.get(neon, 'email')).toBeUndefined();

		// A host that shares no address at all leaves email waiting on one.
		await signIn({}, { optIn: true });
		expect(await channels.get(neon, 'email')).toMatchObject({ status: 'paused', address: null });
	});

	it('turns email off from its unsubscribe link once, forgetting the address', async () => {
		await signIn({ email: 'neon@example.test', emailConfirmed: true }, { optIn: true });
		const token = (await channels.get(neon, 'email'))?.unsubscribeToken ?? '';

		expect(await channels.hasToken(token)).toBe(true);
		expect(await channels.hasToken('made-up')).toBe(false);
		expect(await channels.unsubscribe(token)).toBe(true);
		expect(await channels.get(neon, 'email')).toBeUndefined();
		expect(await channels.unsubscribe(token)).toBe(false);
		expect(await channels.hasToken(token)).toBe(false);
	});
});
