import { DJ_PROFILE, DJ_SET, type DjActivity, type DjProfile, type DjSet } from '#lib/dj.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import type { AccountView } from '#lib/types.ts';
import { z } from 'zod';
import type { Logger } from '../logger';

/** Where an account's PDS is; the app's IdentityResolver. */
type PdsResolver = { resolve(did: string): Promise<{ pds: string }> };

/** How long an account's records are reused before they're fetched again. */
const FRESH_MS = 5 * 60_000;

const CACHE_SIZE = 200;

/** What the PDS answers; the records inside are parsed by the DJ schemas. */
const getRecordOutput = z.object({ value: z.unknown() });

const listRecordsOutput = z.object({ records: z.array(z.object({ uri: z.string(), value: z.unknown() })) });

/** Sets shown per account: the most recent ones. */
const SET_LIMIT = 25;

/** Leaves out accounts with neither stats nor sets on file. */
const onFile = (all: DjActivity[]) => all.filter((entry) => entry.profile || entry.sets.length);

/**
 * DJs' own records, read on demand from their PDS when a page needs them: the playing stats and
 * sets locallibrary publishes. Nothing is stored; a short-lived cache in memory keeps a busy
 * profile from asking the same PDS again within a few minutes. A PDS that's down or slow gives an
 * account with nothing on file, never an error page.
 */
export class DjService {
	private readonly cache = new Map<string, { at: number; value: DjActivity }>();

	constructor(
		private readonly identity: PdsResolver,
		private readonly logger: Logger,
		private readonly fetchImpl: typeof fetch = fetch,
		private readonly now: () => number = Date.now
	) {}

	/** Each account's stats and recent sets, in the order given, leaving out accounts with neither. */
	async activity(accounts: AccountView[]): Promise<DjActivity[]> {
		return onFile(await Promise.all(accounts.map((account) => this.forAccount(account))));
	}

	/**
	 * The same, without waiting, when every account's records are fresh in the cache (always, for no
	 * accounts), so a page can render them with the rest of it instead of streaming them in.
	 */
	fresh(accounts: AccountView[]): DjActivity[] | undefined {
		const all = accounts.map((account) => this.cached(account));

		return all.every((entry): entry is DjActivity => entry !== undefined) ? onFile(all) : undefined;
	}

	private cached(account: AccountView): DjActivity | undefined {
		const cached = this.cache.get(account.did);

		return cached && this.now() - cached.at < FRESH_MS ? { ...cached.value, handle: account.handle } : undefined;
	}

	private async forAccount(account: AccountView): Promise<DjActivity> {
		const cached = this.cached(account);

		if (cached) return cached;

		const [profile, sets] = await Promise.all([this.profile(account.did), this.sets(account.did)]);
		const value = { did: account.did, handle: account.handle, profile, sets };

		this.cache.set(account.did, { at: this.now(), value });

		if (this.cache.size > CACHE_SIZE) this.cache.delete(this.cache.keys().next().value!);

		return value;
	}

	private async profile(did: string): Promise<DjProfile | null> {
		const body = await this.get(
			did,
			'com.atproto.repo.getRecord',
			{ repo: did, collection: DJ_PROFILE, rkey: 'self' },
			getRecordOutput
		);

		if (!body) return null;

		const parsed = djProfileSchema.safeParse(body.value);

		if (!parsed.success) this.logger.debug({ did, issues: parsed.error.issues.length }, 'dj profile does not parse');

		return parsed.success ? parsed.data : null;
	}

	private async sets(did: string): Promise<DjSet[]> {
		const body = await this.get(
			did,
			'com.atproto.repo.listRecords',
			{ repo: did, collection: DJ_SET, limit: String(SET_LIMIT) },
			listRecordsOutput
		);

		return (body?.records ?? [])
			.flatMap((record) => {
				const parsed = djSetSchema.safeParse(record.value);

				return parsed.success ? [{ ...parsed.data, uri: record.uri }] : [];
			})
			.sort((a, b) => b.date.localeCompare(a.date));
	}

	/** A public XRPC query against the account's PDS, parsed, or null when it has nothing or can't be reached. */
	private async get<T>(
		did: string,
		nsid: string,
		params: Record<string, string>,
		output: z.ZodType<T>
	): Promise<T | null> {
		try {
			const url = new URL(`/xrpc/${nsid}`, (await this.identity.resolve(did)).pds);

			for (const [name, value] of Object.entries(params)) url.searchParams.set(name, value);
			const response = await this.fetchImpl(url, { signal: AbortSignal.timeout(5_000) });

			if (!response.ok) {
				// 400 is how a PDS says there's no such record (RecordNotFound).
				if (response.status !== 400)
					this.logger.warn({ did, nsid, status: response.status }, 'dj records request failed');

				return null;
			}

			const parsed = output.safeParse(await response.json());

			if (!parsed.success) this.logger.warn({ did, nsid }, 'dj records response does not parse');

			return parsed.success ? parsed.data : null;
		} catch (cause) {
			this.logger.warn({ err: cause, did, nsid }, 'could not read dj records');

			return null;
		}
	}
}
