import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { load as profilePage } from '../../src/routes/@[handle]/+page.server.ts';
import { callRoute } from '../../src/lib/testing/route-event.ts';
import { TestNetwork } from './network.ts';

const BERLIN = { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

/** Past the half hour an account's status is kept for, so the next check asks its PDS again. */
const LATER_MS = 31 * 60_000;

describe('an account that stops being active, on a real PDS', () => {
	let net: TestNetwork;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'neon'], { admins: ['ada'] });
		const neon = await net.viewer('neon');

		await net.app.profiles.save(neon, {
			basics: { name: 'Neon', location: BERLIN },
			performer: { genres: ['techno'] },
			avatar: 'keep',
			swap: null
		});
		await net.app.djListings.list(await net.viewer('ada'), neon.did);
	});
	afterAll(async () => {
		await net?.close();
	});

	/** The DJs Find DJs lists, by DID. */
	const listed = async () =>
		(await net.app.performers.search({})).items.flatMap((item) => (item.kind === 'dj' ? [item.dj.profile.did] : []));

	/** Checks the directory's accounts again as if half an hour had passed, so each one's PDS is asked. */
	const checkLater = async () => {
		const now = Date.now();
		vi.spyOn(Date, 'now').mockReturnValue(now + LATER_MS);

		try {
			await net.app.accountStatuses.refresh();
		} finally {
			vi.restoreAllMocks();
		}
	};

	it("leaves a deactivated DJ out of Find DJs and says their page isn't active, until they come back", async () => {
		const neon = net.user('neon');

		const page = () =>
			callRoute(profilePage, { app: net.app, params: { handle: neon.handle }, url: `/@${neon.handle}` });

		await neon.agent.com.atproto.server.deactivateAccount({});
		await net.app.accountStatuses.refresh();

		expect(await listed()).toEqual([]);
		await expect(page()).rejects.toMatchObject({ status: 404, body: { title: "This account isn't active" } });

		await neon.agent.com.atproto.server.activateAccount();
		await checkLater();

		expect(await listed()).toEqual([neon.did]);
		await expect(page()).resolves.toMatchObject({ name: 'Neon' });
	});
});
