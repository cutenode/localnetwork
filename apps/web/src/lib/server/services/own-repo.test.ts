import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createTestServices, ROOT } from '../testing/core';
import { spaceWriter } from './own-repo';

const DJ = 'did:plc:dj';

const LEGACY = 'did:plc:legacy';

describe('spaceWriter', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ capable: [DJ] });
	});
	afterEach(async () => {
		await t.db.destroy();
	});

	const writer = async (actor: Parameters<typeof spaceWriter>[1]) => {
		const { client, onBehalf } = await spaceWriter(t.core, actor);

		return { did: client.did, onBehalf };
	};

	it("writes with the actor's own session when their PDS has spaces, and as the root otherwise", async () => {
		expect(await writer(await t.viewer(DJ))).toEqual({ did: DJ, onBehalf: false });
		expect(await writer(await t.viewer(LEGACY))).toEqual({ did: ROOT, onBehalf: true });
	});

	it('writes as the root for an account with spaces whose session lapsed', async () => {
		const lapsed = { ...(await t.viewer(LEGACY)), spacesCapable: true };

		expect(await writer(lapsed)).toEqual({ did: ROOT, onBehalf: true });
	});
});
