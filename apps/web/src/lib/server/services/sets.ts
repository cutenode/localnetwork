import { createHash } from 'node:crypto';
import PQueue from 'p-queue';
import { COLLECTION } from '#lib/nsid.ts';
import type { DjSetView, ProjectProfile, StrongRef } from '#lib/types.ts';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, UserError } from '../core';
import { type HappyViewClient, HappyViewError } from '../happyview/client';

/** How long the set picker waits for HappyView to read an account's sets from its PDS. */
const BACKFILL_WAIT_MS = 5_000;

/** How often one account's sets are re-read at most. Jetstream keeps them current in between. */
const BACKFILL_EVERY_MS = 10 * 60_000;

/** How many accounts' re-reads start at once after a new set script. */
const SCRIPT_REFRESH_CONCURRENCY = 4;

/** The app database key holding the version (a hash) of the set script every manager's sets were re-read for. */
const SCRIPT_REFRESH_KEY = 'happyview_set_script_refreshed';

/** The most sets the picker offers. */
const PICKER_LIMIT = 100;

const sameRef = (a: StrongRef | undefined, b: StrongRef) => a?.uri === b.uri && a.cid === b.cid;

/**
 * DJ sets (`computer.localnetwork.dj.set`) that a project's mixes link. They live in the managers' own repos, and
 * HappyView keeps the ones by accounts that manage a project (lua/computer/localnetwork/dj/set.lua), so every read
 * goes through it.
 */
export class SetService {
	/**
	 * When each account's sets were last re-read, and that re-read, so reopening the picker doesn't crawl their repo
	 * again, and a picker opened while one is running waits for it instead of listing what HappyView had before.
	 */
	private readonly backfilled = new Map<string, { at: number; done: Promise<void> }>();

	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient
	) {}

	/**
	 * The sets `accounts` published, most recently played first, for the project form's picker, or null when
	 * HappyView can't answer (the picker then keeps the sets already linked). Jetstream only brings sets published
	 * after HappyView started keeping an account's, so it first refreshes them.
	 */
	async published(accounts: string[]): Promise<DjSetView[] | null> {
		if (accounts.length === 0) return [];
		await this.refresh(accounts);

		try {
			return (await this.directory.query('listSets', { actors: accounts, limit: PICKER_LIMIT })).sets;
		} catch (error) {
			if (!(error instanceof HappyViewError)) throw error;
			this.core.logger.warn({ err: error }, 'could not list sets for the picker');

			return null;
		}
	}

	/**
	 * The linked sets HappyView has, by URI, for a page to stream: null when HappyView can't answer, so the page
	 * falls back to the links alone.
	 */
	async linked(refs: StrongRef[]): Promise<Map<string, DjSetView> | null> {
		try {
			return await this.byUri(refs);
		} catch (error) {
			if (!(error instanceof HappyViewError)) throw error;
			this.core.logger.warn({ err: error }, 'could not look up linked sets');

			return null;
		}
	}

	/** The linked sets HappyView has, by URI. A profile has at most 12 mixes, so one lookup takes them all. */
	async byUri(refs: StrongRef[]): Promise<Map<string, DjSetView>> {
		const uris = [...new Set(refs.map((ref) => ref.uri))];

		if (uris.length === 0) return new Map();
		const { sets } = await this.directory.query('getSets', { uris });

		return new Map(sets.map((set) => [set.uri, set]));
	}

	/**
	 * Checks the sets `profile` links before root publishes it: each must be a set HappyView has, by one of
	 * `managers`. A link `published` already has, to the same version, stays as it is.
	 */
	async assertLinkable(
		profile: ProjectProfile,
		published: ProjectProfile | undefined,
		managers: string[]
	): Promise<void> {
		const kept = published?.mixes ?? [];

		const linked = (profile.mixes ?? []).flatMap((mix) =>
			mix.set && !kept.some((other) => sameRef(other.set, mix.set!)) ? [{ title: mix.title, set: mix.set }] : []
		);

		if (linked.length === 0) return;

		for (const { title, set } of linked) {
			const parts = parsePublicRecordUri(set.uri);

			if (parts?.collection !== COLLECTION.djSet) throw new UserError(`“${title}” links something that isn't a set`);

			if (!managers.includes(parts.did))
				throw new UserError(`“${title}” links a set by someone who doesn't manage this project`);
		}

		let found: Map<string, DjSetView>;

		try {
			found = await this.byUri(linked.map((mix) => mix.set));
		} catch (error) {
			if (!(error instanceof HappyViewError)) throw error;
			this.core.logger.warn({ err: error }, 'could not check linked sets');
			throw new UserError("Couldn't check the linked sets. Try again in a minute.", 503);
		}

		const missing = linked.find((mix) => !found.has(mix.set.uri));

		if (missing) throw new UserError(`The set linked to “${missing.title}” isn't published anymore`);
	}

	/**
	 * Has HappyView re-read each account's sets from its PDS (when this app has an API key, and at most every ten
	 * minutes per account), for sets Jetstream never brought: ones published before the account managed a project.
	 */
	async refresh(accounts: string[]): Promise<void> {
		await Promise.all(accounts.map((did) => this.backfill(did)));
	}

	private async backfill(did: string): Promise<void> {
		if (!this.directory.admin) return;
		const last = this.backfilled.get(did);

		if (last && Date.now() - last.at < BACKFILL_EVERY_MS) return last.done;
		const done = this.read(did);
		this.backfilled.set(did, { at: Date.now(), done });

		return done;
	}

	/**
	 * Has HappyView re-read every manager's sets once for each version of its set script (`script`), which only applies
	 * to sets HappyView reads after it. The version is kept in the app's database once every re-read has started, so
	 * a failure or a restart before then tries again, and a later boot doesn't. `uploaded` says HappyView was just
	 * given the script, so it needs the re-reads even if this app made them for the same version before. Starts a few
	 * at a time and doesn't wait for them to finish.
	 */
	async refreshForScript(script: string, uploaded: boolean, managers: () => Promise<string[]>): Promise<void> {
		const version = createHash('sha256').update(script).digest('hex');
		const kv = this.core.db;

		if (uploaded) await kv.deleteFrom('kv').where('key', '=', SCRIPT_REFRESH_KEY).execute();
		const done = await kv.selectFrom('kv').select('value').where('key', '=', SCRIPT_REFRESH_KEY).executeTakeFirst();

		if (done?.value === version) return;
		const accounts = await managers();
		const queue = new PQueue({ concurrency: SCRIPT_REFRESH_CONCURRENCY });
		this.core.logger.info({ accounts: accounts.length }, "having HappyView re-read managers' sets");

		const started = await Promise.all(
			accounts.map((did) =>
				queue.add(async () => {
					try {
						await this.directory.backfill(COLLECTION.djSet, did);

						return true;
					} catch (error) {
						this.core.logger.warn({ err: error, did }, 'could not have HappyView re-read sets');

						return false;
					}
				})
			)
		);

		if (!started.every(Boolean)) return;
		await kv
			.insertInto('kv')
			.values({ key: SCRIPT_REFRESH_KEY, value: version })
			.onConflict((conflict) => conflict.column('key').doUpdateSet({ value: version }))
			.execute();
	}

	private async read(did: string): Promise<void> {
		try {
			const job = await this.directory.backfill(COLLECTION.djSet, did);
			const status = await this.directory.awaitBackfill(job, BACKFILL_WAIT_MS);

			if (status === 'failed' || status === 'cancelled') throw new Error(`HappyView's re-read ${status}`);

			if (status !== 'completed') this.core.logger.info({ did, status }, 'HappyView is still reading sets');
		} catch (error) {
			// The picker still lists what HappyView has, and the next open tries again.
			this.backfilled.delete(did);
			this.core.logger.warn({ err: error, did }, 'could not have HappyView read sets');
		}
	}
}
