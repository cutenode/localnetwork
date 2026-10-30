import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { HappyViewClient, HappyViewError } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { createTestServices } from '../testing/core';
import { ProfileService } from './profiles';

const DJ = 'did:plc:dj';

describe('ProfileService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices();
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const write = (basics: JsonObject) =>
		t.net.client(DJ).putRecord({
			collection: COLLECTION.profile,
			rkey: 'self',
			record: {
				$type: COLLECTION.profile,
				basics,
				createdAt: '2026-10-30T00:00:00.000Z',
				updatedAt: '2026-10-30T00:00:00.000Z'
			}
		});

	it('reads a profile with its avatar through this site, and none for an account without one', async () => {
		const avatar = await t.net.client(DJ).uploadBlob(new TextEncoder().encode('an avatar'), 'image/png');
		const { cid } = await write({ name: 'Neon Tide', avatar });

		expect(await t.profiles.get(DJ)).toMatchObject({
			did: DJ,
			uri: `at://${DJ}/${COLLECTION.profile}/self`,
			cid,
			record: { basics: { name: 'Neon Tide' } },
			avatarUrl: `/media/${DJ}/${avatar.ref.$link}`
		});
		expect(await t.profiles.get('did:plc:nobody')).toBeNull();
	});

	it("reads a profile the lexicon doesn't allow as none", async () => {
		// The record script checks types, not lengths, so HappyView keeps it.
		await write({ name: 'n'.repeat(65) });

		expect(await t.profiles.get(DJ)).toBeNull();
	});

	it('fails while the directory is down, rather than reading as no profile', async () => {
		const down = new HappyViewClient({
			url: 'http://happyview.test',
			fetch: async () => new Response(null, { status: 503 })
		});

		const profiles = new ProfileService(new IndexedRepo(down, silentLogger, 0), silentLogger);

		await expect(profiles.get(DJ)).rejects.toThrow(HappyViewError);
	});
});
