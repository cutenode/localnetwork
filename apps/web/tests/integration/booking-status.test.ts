import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { COLLECTION } from '../../src/lib/nsid.ts';
import { TestNetwork } from './network.ts';

const FAR_FUTURE = '2099-01-01T00:00:00.000Z';

describe('booking status from a modified client on a real atproto spaces network', () => {
	let net: TestNetwork;
	let projectUri: string;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'neon', 'pat'], { admins: ['ada'] });

		const project = await net.app.projectAdmin.create({
			actor: await net.viewer('ada'),
			profile: {
				name: 'Neon',
				kind: 'solo',
				genres: ['house'],
				location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }
			},
			managers: [net.user('neon').did]
		});

		projectUri = project.uri;
	});
	afterAll(async () => {
		await net?.close();
	});

	const start = async () =>
		net.app.bookings.start({
			actor: await net.viewer('pat'),
			projectUri,
			show: { title: 'Night' },
			message: 'Free on the 1st?'
		});

	/** Writes straight to `name`'s PDS, past the site's checks, as a client of their own would. */
	const writeStatus = (name: string, space: string, status: string, createdAt: string) =>
		net.user(name).client.createSpaceRecord({
			space,
			collection: COLLECTION.bookingStatus,
			record: { $type: COLLECTION.bookingStatus, status, createdAt }
		});

	const statusOf = async (space: string) => {
		await net.settle();

		return (await net.app.bookings.thread(await net.viewer('pat'), space)).thread.status;
	};

	it("won't let a promoter's client reopen a declined thread", async () => {
		const space = await start();
		await net.app.bookings.setStatus({ actor: await net.viewer('neon'), space, status: 'declined' });

		await writeStatus('pat', space, 'negotiating', FAR_FUTURE);
		await writeStatus('pat', space, 'confirmed', FAR_FUTURE);
		await writeStatus('neon', space, 'open', FAR_FUTURE);

		expect(await statusOf(space)).toBe('declined');
	});

	it("takes a move the rules allow from someone's own client", async () => {
		const space = await start();

		await writeStatus('neon', space, 'negotiating', new Date().toISOString());

		expect(await statusOf(space)).toBe('negotiating');
	});
});
