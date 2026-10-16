import { MIX_REPORT_MAX_BYTES, type MixReport } from '#lib/mix/report.ts';
import { parseMixReportText } from '#lib/mix/schema.ts';
import type { BlobRefJson } from '#lib/types.ts';
import { type Core, UserError } from '../core';

const CID = /^[a-z2-7]{20,100}$/;

/** Parsed reports kept in memory: blobs are immutable, so a CID's report never changes. */
const CACHE_SIZE = 64;

/**
 * Mix tracklists: locallibrary mix reports attached to a project's mixes. Published ones live in
 * the root account's repo; ones proposed in a moderation request live in the requester's repo in
 * the moderation space until approval copies them over.
 */
export class TracklistService {
	private readonly cache = new Map<string, MixReport>();

	constructor(private readonly core: Core) {}

	private remember(cid: string, report: MixReport): MixReport {
		this.cache.set(cid, report);

		if (this.cache.size > CACHE_SIZE) this.cache.delete(this.cache.keys().next().value!);

		return report;
	}

	private parse(cid: string, bytes: Uint8Array): MixReport {
		const result = parseMixReportText(bytes);

		if (!result.ok) {
			this.core.logger.warn({ cid, error: result.error }, 'stored tracklist does not validate');
			throw new UserError('This tracklist could not be read.', 502);
		}

		return this.remember(cid, result.report);
	}

	/** A published tracklist, from the root account's repo. */
	async published(ref: BlobRefJson): Promise<MixReport> {
		const cid = ref.ref.$link;

		if (!CID.test(cid)) throw new UserError('Not a tracklist', 404);
		const cached = this.cache.get(cid);

		if (cached) return cached;

		if (!this.core.fetchBlob) throw new UserError('Tracklists are unavailable.', 503);
		let bytes: Uint8Array;

		try {
			({ bytes } = await this.core.fetchBlob(this.core.rootDid, cid, MIX_REPORT_MAX_BYTES));
		} catch (error) {
			this.core.logger.warn({ err: error, cid }, 'could not fetch tracklist');
			throw new UserError('This tracklist could not be fetched right now.', 502);
		}

		return this.parse(cid, bytes);
	}

	/**
	 * A tracklist proposed in a moderation request (for reviewers). A mix that kept its published
	 * tracklist points at the root's copy instead.
	 */
	async proposed(repoDid: string, ref: BlobRefJson): Promise<MixReport> {
		const cid = ref.ref.$link;

		if (!CID.test(cid)) throw new UserError('Not a tracklist', 404);
		const cached = this.cache.get(cid);

		if (cached) return cached;

		if (repoDid !== this.core.rootDid && this.core.fetchSpaceBlob) {
			try {
				const { bytes } = await this.core.fetchSpaceBlob(this.core.spaces.moderation, repoDid, cid);

				if (bytes.length <= MIX_REPORT_MAX_BYTES) return this.parse(cid, bytes);
			} catch (error) {
				if (error instanceof UserError) throw error;
				this.core.logger.info({ err: error, cid }, "tracklist isn't in the requester's repo; trying the published one");
			}
		}

		return this.published(ref);
	}
}
