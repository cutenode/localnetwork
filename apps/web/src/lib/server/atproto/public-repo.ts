import { z } from 'zod';
import type { Logger } from '../logger';

/** Where an account's PDS is; the app's IdentityResolver. */
export type PdsResolver = { resolve(did: string): Promise<{ pds: string }> };

const recordOutput = z.object({ uri: z.string(), cid: z.string().optional(), value: z.record(z.string(), z.json()) });

const listOutput = z.object({ records: z.array(recordOutput), cursor: z.string().optional() });

export type PublicRecord = z.infer<typeof recordOutput>;

/**
 * Reads public records straight from an account's PDS with unauthenticated XRPC. A PDS that's down,
 * slow or answers oddly reads as having nothing, never as an error, so a page that shows someone's
 * records still renders; callers parse what comes back.
 */
export class PublicRepo {
	constructor(
		private readonly identity: PdsResolver,
		private readonly logger: Logger,
		private readonly fetchImpl: typeof fetch = fetch
	) {}

	async getRecord(did: string, collection: string, rkey: string): Promise<PublicRecord | null> {
		return this.query(did, 'com.atproto.repo.getRecord', { repo: did, collection, rkey }, recordOutput);
	}

	async listRecords(
		did: string,
		collection: string,
		options: { limit?: number; cursor?: string } = {}
	): Promise<{ records: PublicRecord[]; cursor?: string }> {
		const params = { repo: did, collection, limit: String(options.limit ?? 100), cursor: options.cursor };

		return (await this.query(did, 'com.atproto.repo.listRecords', params, listOutput)) ?? { records: [] };
	}

	private async query<T>(
		did: string,
		nsid: string,
		params: Record<string, string | undefined>,
		output: z.ZodType<T>
	): Promise<T | null> {
		try {
			const url = new URL(`/xrpc/${nsid}`, (await this.identity.resolve(did)).pds);

			for (const [name, value] of Object.entries(params)) if (value !== undefined) url.searchParams.set(name, value);
			const response = await this.fetchImpl(url, { signal: AbortSignal.timeout(5_000) });

			if (!response.ok) {
				// 400 is how a PDS says there's no such record (RecordNotFound).
				if (response.status !== 400)
					this.logger.warn({ did, nsid, status: response.status }, 'public repo request failed');

				return null;
			}

			const parsed = output.safeParse(await response.json());

			if (!parsed.success) this.logger.warn({ did, nsid }, 'public repo response does not parse');

			return parsed.success ? parsed.data : null;
		} catch (cause) {
			this.logger.warn({ err: cause, did, nsid }, 'could not read a public repo');

			return null;
		}
	}
}
