import { afterEach, beforeEach, expect, it } from 'vitest';
import { type Db, openDatabase } from '#lib/server/db/index.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { NotificationChannels } from '#lib/server/notifications/channels.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const DJ = 'did:plc:dj';

let db: Db;

let channels: NotificationChannels;

beforeEach(async () => {
	db = await openDatabase(':memory:');
	channels = new NotificationChannels({
		db,
		logger: silentLogger,
		limits: {
			account: new RateLimiter(5, 5 / 86_400),
			accountAddress: new RateLimiter(3, 3 / 86_400),
			address: new RateLimiter(10, 10 / 86_400),
			all: new RateLimiter(100, 100 / 3600)
		}
	});
});

afterEach(async () => {
	await db.destroy();
});

/** DJ turns email on, and the unsubscribe token every email they get carries. */
async function emailOn(): Promise<string> {
	await channels.onSignIn({
		did: DJ,
		scope: 'atproto account:email',
		optIn: true,
		session: async () => ({ email: 'dj@example.com', emailConfirmed: true })
	});
	const row = await channels.get(DJ, 'email');

	return row!.unsubscribeToken!;
}

const app = () => ({ notificationChannels: channels });

it('asks before turning email off from the link in an email', async () => {
	const token = await emailOn();

	expect(await loadRoute<PageServerData>(load, { app: app(), url: `/notifications/unsubscribe?t=${token}` })).toEqual({
		token,
		valid: true
	});
	expect(await channels.get(DJ, 'email')).toBeDefined();
});

it('says a link with no token, or one that turns nothing off, is not valid', async () => {
	await emailOn();

	expect(await loadRoute<PageServerData>(load, { app: app(), url: '/notifications/unsubscribe' })).toEqual({
		token: '',
		valid: false
	});
	expect(await loadRoute<PageServerData>(load, { app: app(), url: '/notifications/unsubscribe?t=gone' })).toEqual({
		token: 'gone',
		valid: false
	});
});

it('turns email off and forgets the address once the button is pressed', async () => {
	const token = await emailOn();

	expect(
		await callRoute(actions.default, { app: app(), url: '/notifications/unsubscribe', form: { t: token } })
	).toEqual({
		done: true
	});
	expect(await channels.get(DJ, 'email')).toBeUndefined();
});

it('turns nothing off without a token', async () => {
	await emailOn();

	expect(await callRoute(actions.default, { app: app(), url: '/notifications/unsubscribe', form: { t: ' ' } })).toEqual(
		{
			done: false
		}
	);
	expect(await channels.get(DJ, 'email')).toBeDefined();
});
