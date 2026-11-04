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

/** The confirmation links sent, by address. */
let sent: Map<string, string>;

beforeEach(async () => {
	db = await openDatabase(':memory:');
	sent = new Map();
	channels = new NotificationChannels({
		db,
		logger: silentLogger,
		sendConfirmation: async (address, token) => {
			sent.set(address, token);
		},
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

/** DJ turns email on with an address their host confirmed, which sends them a link; returns the link's token. */
async function turnEmailOn(): Promise<string> {
	await channels.onSignIn({
		did: DJ,
		scope: 'atproto account:email',
		optIn: true,
		session: async () => ({ email: 'dj@example.com', emailConfirmed: true })
	});

	return sent.get('dj@example.com')!;
}

/** DJ's email setting. */
const emailStatus = async () => (await channels.get(DJ, 'email'))?.status;

it('asks before confirming the address a link went to', async () => {
	const token = await turnEmailOn();

	expect(
		await loadRoute<PageServerData>(load, {
			app: { notificationChannels: channels },
			url: `/notifications/confirm?t=${token}`
		})
	).toEqual({
		token,
		state: 'valid'
	});
	expect(await emailStatus()).toBe('paused');
});

it('says a link without a token is not one', async () => {
	await turnEmailOn();

	expect(
		await loadRoute<PageServerData>(load, { app: { notificationChannels: channels }, url: '/notifications/confirm' })
	).toEqual({
		token: '',
		state: 'invalid'
	});
});

it('turns email on once the button is pressed, and the link stops working', async () => {
	const token = await turnEmailOn();

	const confirm = () =>
		callRoute(actions.default, {
			app: { notificationChannels: channels },
			url: '/notifications/confirm',
			form: { t: token }
		});

	expect(await confirm()).toEqual({ result: 'confirmed' });
	expect(await emailStatus()).toBe('active');
	expect(await confirm()).toEqual({ result: 'invalid' });
});
