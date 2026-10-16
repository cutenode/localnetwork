import { DJ_PROFILE, DJ_SET, type DjActivity, type DjProfile, type DjSet } from '#lib/dj.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import type { AccountView } from '#lib/types.ts';
import type { PublicRepo } from '../atproto/public-repo';
import type { Logger } from '../logger';

/** How long an account's records are reused before they're fetched again. */
const FRESH_MS = 5 * 60_000;

const CACHE_SIZE = 200;

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
		private readonly repos: PublicRepo,
		private readonly logger: Logger,
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
		const body = await this.repos.getRecord(did, DJ_PROFILE, 'self');

		if (!body) return null;

		const parsed = djProfileSchema.safeParse(body.value);

		if (!parsed.success) this.logger.debug({ did, issues: parsed.error.issues.length }, 'dj profile does not parse');

		return parsed.success ? parsed.data : null;
	}

	private async sets(did: string): Promise<DjSet[]> {
		const { records } = await this.repos.listRecords(did, DJ_SET, { limit: SET_LIMIT });

		return records
			.flatMap((record) => {
				const parsed = djSetSchema.safeParse(record.value);

				return parsed.success ? [{ ...parsed.data, uri: record.uri }] : [];
			})
			.sort((a, b) => b.date.localeCompare(a.date));
	}
}
