import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { MIX_REPORT_MAX_BYTES, type MixReport } from '#lib/mix/report.ts';
import { sampleReport } from '#lib/mix/sample.ts';
import type { BlobRefJson } from '#lib/types.ts';
import type { Core } from '../core';
import { createTestServices, ROOT } from '../testing/core';
import { TracklistService } from './tracklists';

const DJ = 'did:plc:dj';

const titled = (title: string): MixReport => ({ ...sampleReport, mix: { ...sampleReport.mix, title } });

const notFetched = { status: 502, message: 'This tracklist could not be fetched right now.' };

describe('TracklistService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices();
	});
	afterEach(async () => {
		await t.db.destroy();
	});

	const tracklists = (overrides: Partial<Core> = {}) => new TracklistService({ ...t.core, ...overrides });

	/** Uploads `value` as JSON to `did`'s repo, as a tracklist blob. */
	const upload = async (value: MixReport | JsonObject, did = ROOT): Promise<BlobRefJson> =>
		t.net.client(did).uploadBlob(new TextEncoder().encode(JSON.stringify(value)), 'application/json');

	const forget = (ref: BlobRefJson) => t.net.blobs.delete(ref.ref.$link);

	describe('published', () => {
		it("reads a tracklist from the root's repo once, then from memory", async () => {
			const service = tracklists();
			const ref = await upload(titled('Warehouse'));

			expect((await service.published(ref)).mix.title).toBe('Warehouse');
			forget(ref);
			expect((await service.published(ref)).mix.title).toBe('Warehouse');
			await expect(tracklists().published(ref)).rejects.toMatchObject(notFetched);
		});

		it('keeps only the 64 latest tracklists in memory', async () => {
			const service = tracklists();
			const refs = await Promise.all(Array.from({ length: 65 }, (_, i) => upload(titled(`Mix ${i}`))));

			for (const ref of refs) await service.published(ref);
			refs.forEach(forget);
			expect((await service.published(refs[1])).mix.title).toBe('Mix 1');
			await expect(service.published(refs[0])).rejects.toMatchObject(notFetched);
		});

		it("refuses a ref that isn't a CID, and says when tracklists can't be fetched or read", async () => {
			const ref = await upload(titled('Warehouse'));
			const broken = await upload({ format: 'nope' });

			await expect(tracklists().published({ ...ref, ref: { $link: '../etc/passwd' } })).rejects.toMatchObject({
				status: 404,
				message: 'Not a tracklist'
			});
			await expect(tracklists({ fetchBlob: undefined }).published(ref)).rejects.toMatchObject({
				status: 503,
				message: 'Tracklists are unavailable.'
			});
			await expect(tracklists().published(broken)).rejects.toMatchObject({
				status: 502,
				message: 'This tracklist could not be read.'
			});
		});
	});

	describe('proposed', () => {
		it("reads a tracklist from the requester's repo in the moderation space, then from memory", async () => {
			const asked: string[][] = [];
			const ref = await upload(titled('Proposed'), DJ);

			const service = tracklists({
				fetchSpaceBlob: async (space, repo, cid) => {
					asked.push([space, repo, cid]);

					return t.core.fetchSpaceBlob!(space, repo, cid);
				}
			});

			expect((await service.proposed(DJ, ref)).mix.title).toBe('Proposed');
			expect((await service.proposed(DJ, ref)).mix.title).toBe('Proposed');
			expect(asked).toEqual([[t.core.spaces.moderation, DJ, ref.ref.$link]]);
		});

		it("falls back to the published copy when the requester's repo doesn't have it or it's too big", async () => {
			const ref = await upload(titled('Published'));
			const missing = tracklists({ fetchSpaceBlob: () => Promise.reject(new Error('missing blob')) });

			const huge = tracklists({
				fetchSpaceBlob: async () => ({ bytes: new Uint8Array(MIX_REPORT_MAX_BYTES + 1), mimeType: 'application/json' })
			});

			expect((await missing.proposed(DJ, ref)).mix.title).toBe('Published');
			expect((await huge.proposed(DJ, ref)).mix.title).toBe('Published');
		});

		it("reads the root's own tracklists from its repo, and passes on one that can't be read", async () => {
			const unreachable = () => Promise.reject(new Error('the root is not in its own moderation space'));
			const ref = await upload(titled('Published'));
			const broken = await upload({ format: 'nope' }, DJ);

			expect((await tracklists({ fetchSpaceBlob: unreachable }).proposed(ROOT, ref)).mix.title).toBe('Published');
			expect((await tracklists({ fetchSpaceBlob: undefined }).proposed(DJ, ref)).mix.title).toBe('Published');
			await expect(tracklists().proposed(DJ, broken)).rejects.toMatchObject({
				status: 502,
				message: 'This tracklist could not be read.'
			});
			await expect(tracklists().proposed(DJ, { ...ref, ref: { $link: 'NOT-A-CID' } })).rejects.toMatchObject({
				status: 404
			});
		});
	});
});
