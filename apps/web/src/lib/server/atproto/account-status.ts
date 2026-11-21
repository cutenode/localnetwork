import PQueue from 'p-queue';
import { z } from 'zod';
import type { Logger } from '../logger';
import type { IdentityResolver } from './identity';

/**
 * Whether an account's records show on the site. `deactivated`, `takendown`, `suspended` and `deleted` are the
 * `com.atproto.sync.getRepoStatus` statuses that hide them; any other (`desynchronized`, `throttled`) is a sync
 * condition of an account still there, so it reads as `active`.
 */
export type AccountStatus = 'active' | 'deactivated' | 'takendown' | 'suspended' | 'deleted';

/** Asks where an account lives whether it's active. Throws when it can't tell. */
type StatusLookup = (did: string) => Promise<AccountStatus>;

const INACTIVE = new Set<string>(['deactivated', 'takendown', 'suspended', 'deleted']);

const isInactive = (status: string | undefined): status is Exclude<AccountStatus, 'active'> =>
	status !== undefined && INACTIVE.has(status);

const repoStatus = z.object({ active: z.boolean(), status: z.string().optional() });

const xrpcError = z.object({ error: z.string() });

/** How long one account's status is reused before it's asked again. */
const FRESH_MS = 30 * 60_000;

/** How long asking one account's PDS may take. */
const LOOKUP_TIMEOUT_MS = 5_000;

/** How many accounts are asked at once when the directory's accounts are checked. */
const CONCURRENCY = 8;

/** The most accounts whose status is kept; the oldest go first. */
const KEPT = 10_000;

/** The most inactive accounts a directory query takes (its `inactive` param's maxLength). */
const INACTIVE_PARAM_MAX = 100;

/**
 * `com.atproto.sync.getRepoStatus` from the PDS the account's DID document names, through the guarded fetch. A PDS
 * answers `RepoNotFound` for an account it deleted. The resolver caches DID documents, and the PDS an account just
 * moved away from answers `RepoNotFound` or deactivated, so an inactive answer is checked against a fresh document and,
 * when that names another PDS, asked of that one.
 */
export function repoStatusLookup(identity: Pick<IdentityResolver, 'resolve' | 'serviceApiFetch'>): StatusLookup {
	const statusAt = async (pds: string, did: string): Promise<AccountStatus> => {
		const url = new URL('/xrpc/com.atproto.sync.getRepoStatus', pds);
		url.searchParams.set('did', did);

		const response = await identity.serviceApiFetch(url, {
			redirect: 'error',
			signal: AbortSignal.timeout(LOOKUP_TIMEOUT_MS)
		});

		const body: unknown = await response.json().catch(() => null);

		if (!response.ok) {
			if (xrpcError.safeParse(body).data?.error === 'RepoNotFound') return 'deleted';
			throw new Error(`getRepoStatus: HTTP ${response.status}`);
		}

		const { active, status } = repoStatus.parse(body);

		return !active && isInactive(status) ? status : 'active';
	};

	return async (did) => {
		const { pds } = await identity.resolve(did);
		const status = await statusAt(pds, did);

		if (status === 'active') return status;
		const current = (await identity.resolve(did, true)).pds;

		return current === pds ? status : statusAt(current, did);
	};
}

type AccountStatusOptions = {
	lookup: StatusLookup;
	/** Every account whose records the directory reads: the managers of the root's projects and the DJs staff list. */
	accounts: () => Promise<string[]>;
	logger: Logger;
	/** Called when an account taken down or suspended is active again, since HappyView dropped its records then. */
	reinstated?: (did: string) => void;
	freshMs?: number;
	now?: () => number;
};

/**
 * Which accounts are inactive, asked of their PDS when a page needs to know and kept for half an hour (ADR 0020).
 * HappyView keeps a deactivated account's records, so the directory queries are told which accounts to leave out, and
 * the account's own pages say it isn't active. A PDS that can't be asked leaves the account as it was last seen, or
 * active when it never was: the site fails open. A deleted account is asked again like any other, since one whose
 * PDS changed can look deleted for a moment, and HappyView's tombstone keeps a really deleted one's records out anyway.
 */
export class AccountStatuses {
	private readonly known = new Map<string, { status: AccountStatus; at: number }>();
	private readonly asking = new Map<string, Promise<AccountStatus>>();
	private inDirectory?: { dids: string[]; at: number };
	private refreshing?: Promise<string[]>;
	private readonly freshMs: number;
	private readonly now: () => number;

	constructor(private readonly options: AccountStatusOptions) {
		this.freshMs = options.freshMs ?? FRESH_MS;
		this.now = options.now ?? (() => Date.now());
	}

	/**
	 * `did`'s status. A status seen before answers at once, and is asked again in the background once it's older than
	 * half an hour; only an account never seen waits for its PDS.
	 */
	async status(did: string): Promise<AccountStatus> {
		const known = this.known.get(did);

		if (!known) return this.ask(did);

		if (!this.isFresh(known.at)) void this.ask(did);

		return known.status;
	}

	/**
	 * The directory's accounts last found deactivated, taken down or suspended, for the queries to leave out: none until
	 * the first check is done.
	 * When the answer is older than half an hour, refresh() starts in the background, so no page waits on a PDS.
	 */
	inactive(): string[] {
		const known = this.inDirectory;

		if (!known || !this.isFresh(known.at)) void this.refresh();

		return known?.dids ?? [];
	}

	/** Checks the directory's accounts again now, asking only the ones not asked lately, and joining a check under way. */
	refresh(): Promise<string[]> {
		this.refreshing ??= this.checkDirectory().finally(() => {
			this.refreshing = undefined;
		});

		return this.refreshing;
	}

	private isFresh(at: number): boolean {
		return this.now() - at < this.freshMs;
	}

	private async checkDirectory(): Promise<string[]> {
		let accounts: string[];

		try {
			accounts = [...new Set(await this.options.accounts())];
		} catch (error) {
			// Kept as it was for another half hour, so a directory that's down isn't asked on every page.
			this.options.logger.warn("could not list the directory's accounts to check", { err: error });
			const dids = this.inDirectory?.dids ?? [];
			this.inDirectory = { dids, at: this.now() };

			return dids;
		}

		const queue = new PQueue({ concurrency: CONCURRENCY });

		const statuses = await Promise.all(
			accounts.map((did) =>
				queue.add(() => {
					const known = this.known.get(did);

					return known && this.isFresh(known.at) ? known.status : this.ask(did);
				})
			)
		);

		// HappyView already dropped a deleted account's records, so it isn't sent to take one of the param's places.
		let dids = accounts.filter((_, index) => statuses[index] !== 'active' && statuses[index] !== 'deleted');

		if (dids.length > INACTIVE_PARAM_MAX) {
			this.options.logger.warn('more inactive accounts than the directory queries take', { inactive: dids.length });
			dids = dids.slice(0, INACTIVE_PARAM_MAX);
		}

		this.inDirectory = { dids, at: this.now() };
		this.options.logger.debug("checked the directory's accounts", { accounts: accounts.length, inactive: dids });

		return dids;
	}

	/** Asks `did`'s PDS, once at a time per account, and keeps the answer. */
	private ask(did: string): Promise<AccountStatus> {
		let asking = this.asking.get(did);

		if (!asking) {
			asking = this.lookup(did).finally(() => this.asking.delete(did));
			this.asking.set(did, asking);
		}

		return asking;
	}

	private async lookup(did: string): Promise<AccountStatus> {
		const before = this.known.get(did)?.status;
		let status: AccountStatus;

		try {
			status = await this.options.lookup(did);
		} catch (error) {
			status = before ?? 'active';
			this.options.logger.warn("could not check an account's status", { err: error, did, assumed: status });
		}

		this.known.delete(did);
		this.known.set(did, { status, at: this.now() });

		if (this.known.size > KEPT) this.known.delete(this.known.keys().next().value!);

		if (before !== undefined && status !== before) {
			this.options.logger.info("an account's status changed", { did, from: before, to: status });

			if (status === 'active' && (before === 'takendown' || before === 'suspended')) this.options.reinstated?.(did);
		}

		return status;
	}
}
