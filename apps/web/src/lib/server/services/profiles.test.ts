import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { BSKY_PROFILE, COLLECTION } from '#lib/nsid.ts';
import { ReauthError } from '../core';
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

		const profiles = new ProfileService(t.core, new IndexedRepo(down, silentLogger, 0));

		await expect(profiles.get(DJ)).rejects.toThrow(HappyViewError);
	});
});

describe('ProfileService.save and blueskyPrefill', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ capable: [DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const png = { bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), mimeType: 'image/png' };

	it("saves a new profile to the account's own repo with its picture, and the directory serves it at once", async () => {
		await t.profiles.save(await t.viewer(DJ), {
			basics: { name: 'Neon', location: { city: 'Berlin', country: 'DE' } },
			performer: {},
			avatar: { upload: png },
			swap: null
		});

		const saved = await t.profiles.get(DJ);
		expect(saved?.record).toMatchObject({ basics: { name: 'Neon', avatar: { mimeType: 'image/png' } }, performer: {} });
		expect(saved?.record.promoter).toBeUndefined();
		expect(saved?.avatarUrl).toBe(`/media/${DJ}/${saved?.record.basics.avatar?.ref.$link}`);
		await t.audit.idle();
		expect(await t.audit.list({ action: 'profile.save' })).toMatchObject([
			{ actorDid: DJ, summary: 'Set up their profile' }
		]);
	});

	it('keeps the picture, drops a section turned off, and never saves over a change made since the form opened', async () => {
		const viewer = await t.viewer(DJ);
		await t.profiles.save(viewer, {
			basics: { name: 'Neon' },
			performer: { genres: ['techno'] },
			avatar: { upload: png },
			swap: null
		});
		const first = (await t.profiles.get(DJ))!;

		await t.profiles.save(viewer, { basics: { name: 'Neon Tide' }, promoter: {}, avatar: 'keep', swap: first.cid });
		const second = (await t.profiles.get(DJ))!;
		expect(second.record).toMatchObject({
			basics: { name: 'Neon Tide', avatar: first.record.basics.avatar },
			promoter: {}
		});
		expect(second.record.performer).toBeUndefined();
		expect(second.record.createdAt).toBe(first.record.createdAt);

		await expect(
			t.profiles.save(viewer, { basics: { name: 'Stale' }, avatar: 'keep', swap: first.cid })
		).rejects.toThrow('Your profile changed since you opened it');
		await t.profiles.save(viewer, { basics: { name: 'Neon Tide' }, avatar: 'remove', swap: second.cid });
		expect((await t.profiles.get(DJ))?.record.basics.avatar).toBeUndefined();
	});

	it('asks an account whose session lapsed to sign in again', async () => {
		const viewer = await t.viewer('did:plc:lapsed');

		await expect(t.profiles.save(viewer, { basics: { name: 'Gone' }, avatar: 'keep', swap: null })).rejects.toThrow(
			ReauthError
		);
	});

	it("fills a first profile in from the account's own Bluesky profile, picture included", async () => {
		const client = t.net.client(DJ);
		const avatar = await client.uploadBlob(png.bytes, png.mimeType);

		await client.putRecord({
			collection: BSKY_PROFILE,
			rkey: 'self',
			record: { $type: BSKY_PROFILE, displayName: 'Neon', description: 'Hypnotic techno', avatar }
		});

		expect(await t.profiles.blueskyPrefill(DJ)).toEqual({ name: 'Neon', bio: 'Hypnotic techno', avatar });
		// Without a session there's nothing to read it with.
		expect(await t.profiles.blueskyPrefill('did:plc:lapsed')).toBeNull();
	});
});
