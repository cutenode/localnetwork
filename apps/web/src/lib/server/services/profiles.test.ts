import type { JsonObject } from '@atproto/lex-json';
import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BSKY_PROFILE, COLLECTION } from '#lib/nsid.ts';
import { ReauthError, UserError } from '../core';
import { HappyViewClient, HappyViewError } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { createTestServices } from '../testing/core';
import { FakeRootClient } from '../testing/fakes';
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

	it('skips a profile nested too deep to check, rather than failing the read', () => {
		// About 40 KB of JSON, which the directory serves as it would any record.
		const record = JSON.parse(`{"basics":{"name":"Neon Tide"},"extra":${'['.repeat(20_000)}${']'.repeat(20_000)}}`);

		expect(t.profiles.toView(DJ, `at://${DJ}/${COLLECTION.profile}/self`, 'bafy', record)).toBeNull();
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
		vi.restoreAllMocks();
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

	it("has the directory read the account's Bluesky profile once they have a profile here, for its pronouns", async () => {
		await t.net.client(DJ).putRecord({
			collection: BSKY_PROFILE,
			rkey: 'self',
			record: { $type: BSKY_PROFILE, displayName: 'Neon', pronouns: 'they/them' }
		});
		// Without a profile here, the directory skipped it.
		expect(await t.profiles.bluesky(DJ)).toEqual({ name: null, pronouns: null });

		await t.profiles.save(await t.viewer(DJ), { basics: { name: 'Neon' }, avatar: 'keep', swap: null });
		await expect.poll(() => t.profiles.bluesky(DJ)).toEqual({ name: 'Neon', pronouns: 'they/them' });
	});

	it('asks an account whose session lapsed to sign in again', async () => {
		const viewer = await t.viewer('did:plc:lapsed');

		await expect(t.profiles.save(viewer, { basics: { name: 'Gone' }, avatar: 'keep', swap: null })).rejects.toThrow(
			ReauthError
		);
	});

	it('uploads tracklists for the mixes that come with one, and points the picture at a blob already uploaded', async () => {
		const avatar = await t.net.client(DJ).uploadBlob(png.bytes, png.mimeType);
		const report = { bytes: new TextEncoder().encode('{"tracks":[]}'), mimeType: 'application/json' };

		await t.profiles.save(await t.viewer(DJ), {
			basics: { name: 'Neon' },
			performer: {
				mixes: [
					{ title: 'Late Room', url: 'https://mixes.test/late-room' },
					{ title: 'Plain', url: 'https://mixes.test/plain' }
				]
			},
			tracklists: [report, undefined],
			avatar: { blob: avatar },
			swap: null
		});

		const { record } = (await t.profiles.get(DJ))!;
		expect(record.basics.avatar).toEqual(avatar);
		expect(record.performer?.mixes?.[0].tracklist).toMatchObject({
			mimeType: 'application/json',
			size: report.bytes.length
		});
		expect(record.performer?.mixes?.[1]).toEqual({ title: 'Plain', url: 'https://mixes.test/plain' });
	});

	it('asks for a reload when the PDS refuses a save over a newer version, and passes other failures on', async () => {
		const viewer = await t.viewer(DJ);
		const save = () => t.profiles.save(viewer, { basics: { name: 'Neon' }, avatar: 'keep', swap: null });

		t.net.failWrites(DJ, 1, new XRPCError(400, 'InvalidSwap'));
		await expect(save()).rejects.toThrow(
			new UserError('Your profile changed since you opened it. Reload to see the latest, then save again.')
		);
		t.net.failWrites(DJ);
		await expect(save()).rejects.toThrow('UpstreamFailure');
		expect(await t.profiles.get(DJ)).toBeNull();
	});

	it("fills a first profile in from the account's own Bluesky profile, picture included", async () => {
		const client = t.net.client(DJ);
		const avatar = await client.uploadBlob(png.bytes, png.mimeType);

		await client.putRecord({
			collection: BSKY_PROFILE,
			rkey: 'self',
			record: {
				$type: BSKY_PROFILE,
				displayName: 'Neon',
				description: 'Hypnotic techno',
				pronouns: 'they/them',
				avatar
			}
		});

		expect(await t.profiles.blueskyPrefill(DJ)).toEqual({
			name: 'Neon',
			bio: 'Hypnotic techno',
			pronouns: 'they/them',
			avatar
		});
		// Without a session there's nothing to read it with.
		expect(await t.profiles.blueskyPrefill('did:plc:lapsed')).toBeNull();
	});

	it("offers nothing without a Bluesky profile or when it can't be read, and no picture of a type profiles don't take", async () => {
		const client = t.net.client(DJ);
		expect(await t.profiles.blueskyPrefill(DJ)).toBeNull();

		const gif = await client.uploadBlob(new TextEncoder().encode('GIF89a'), 'image/gif');
		await client.putRecord({
			collection: BSKY_PROFILE,
			rkey: 'self',
			record: { $type: BSKY_PROFILE, displayName: 'N'.repeat(70), avatar: gif }
		});
		expect(await t.profiles.blueskyPrefill(DJ)).toEqual({ name: 'N'.repeat(64), bio: undefined, avatar: undefined });

		vi.spyOn(FakeRootClient.prototype, 'getRecord').mockRejectedValueOnce(new XRPCError(502, 'UpstreamFailure'));
		expect(await t.profiles.blueskyPrefill(DJ)).toBeNull();
	});
});
