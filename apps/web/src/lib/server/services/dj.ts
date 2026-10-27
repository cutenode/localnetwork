import type { JsonObject } from '@atproto/lex-json';
import type { DjActivity, DjProfile, DjSet } from '#lib/dj.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { AccountView } from '#lib/types.ts';
import type { IndexedRepo } from '../happyview/indexed';
import type { Logger } from '../logger';

/** How long an account's records are reused before they're fetched again. */
const FRESH_MS = 5 * 60_000;

const CACHE_SIZE = 200;

/** Sets shown per account: the most recent ones. */
const SET_LIMIT = 25;

/** Leaves out accounts with neither stats nor sets on file. */
const onFile = (all: DjActivity[]) => all.filter((entry) => entry.profile || entry.sets.length);

/** Where a DJ's stats are: the one record of its collection in their repo. */
const profileUri = (did: string) => `at://${did}/${COLLECTION.djProfile}/self`;

/**
 * DJs' own records, read from HappyView when a page needs them: the playing stats and sets locallibrary publishes to
 * their repos (lua/computer/localnetwork/dj/). Nothing is stored here; a short-lived cache in memory keeps a busy
 * profile from asking again within a few minutes. A HappyView that's down gives accounts with nothing on file, never
 * an error page.
 */
export class DjService {
	private readonly cache = new Map<string, { at: number; value: DjActivity }>();

	constructor(
		private readonly repos: IndexedRepo,
		private readonly logger: Logger,
		private readonly now: () => number = Date.now
	) {}

	/** Each account's stats and recent sets, in the order given, leaving out accounts with neither. */
	async activity(accounts: AccountView[]): Promise<DjActivity[]> {
		const cached = accounts.map((account) => this.cached(account));
		const loaded = await this.load(accounts.flatMap((account, i) => (cached[i] ? [] : [account.did])));

		return onFile(accounts.map((account, i) => cached[i] ?? { ...loaded.get(account.did)!, handle: account.handle }));
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

	/** The records of `dids`, fetched together (every stats record in one lookup) and cached. */
	private async load(dids: string[]): Promise<Map<string, DjActivity>> {
		const unique = [...new Set(dids)];

		if (unique.length === 0) return new Map();

		const [profiles, sets] = await Promise.all([
			this.repos.getRecords(unique.map(profileUri)),
			Promise.all(unique.map((did) => this.sets(did)))
		]);

		const at = this.now();

		return new Map(
			unique.map((did, i) => {
				const value = {
					did,
					handle: null,
					profile: this.profile(did, profiles.get(profileUri(did))?.value),
					sets: sets[i]
				};

				this.cache.set(did, { at, value });

				if (this.cache.size > CACHE_SIZE) this.cache.delete(this.cache.keys().next().value!);

				return [did, value] as const;
			})
		);
	}

	private profile(did: string, value: JsonObject | undefined): DjProfile | null {
		if (!value) return null;

		const parsed = djProfileSchema.safeParse(value);

		if (!parsed.success) this.logger.debug({ did, issues: parsed.error.issues.length }, 'dj profile does not parse');

		return parsed.success ? parsed.data : null;
	}

	private async sets(did: string): Promise<DjSet[]> {
		const { records } = await this.repos.listRecords(did, COLLECTION.djSet, { limit: SET_LIMIT });

		return records
			.flatMap((record) => {
				const parsed = djSetSchema.safeParse(record.value);

				return parsed.success ? [{ ...parsed.data, uri: record.uri }] : [];
			})
			.sort((a, b) => b.date.localeCompare(a.date));
	}
}
