import { COLLECTION } from '#lib/nsid.ts';
import type { DjSetView, ProjectProfile, StrongRef } from '#lib/types.ts';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, UserError } from '../core';
import { type HappyViewClient, HappyViewError } from '../happyview/client';

/** How long the set picker waits for HappyView to read an account's sets from its PDS. */
const BACKFILL_WAIT_MS = 5_000;

/** How often one account's sets are re-read at most. Jetstream keeps them current in between. */
const BACKFILL_EVERY_MS = 10 * 60_000;

/** The most sets the picker offers. */
const PICKER_LIMIT = 100;

const sameRef = (a: StrongRef | undefined, b: StrongRef) => a?.uri === b.uri && a.cid === b.cid;

/**
 * DJ sets (`computer.localnetwork.dj.set`) that a project's mixes link. They live in the managers' own repos, and
 * HappyView keeps the ones by accounts that manage a project (lua/computer/localnetwork/dj/set.lua), so every read
 * goes through it.
 */
export class SetService {
	/** When each account's sets were last re-read, so reopening the picker doesn't crawl their repo again. */
	private readonly backfilled = new Map<string, number>();

	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient
	) {}

	/**
	 * The sets `accounts` published, most recently played first, for the project form's picker, or null when
	 * HappyView can't answer (the picker then keeps the sets already linked). Jetstream only brings sets published
	 * after HappyView started keeping an account's, so it first re-reads each account's sets from its PDS (when this
	 * app has an API key, and at most every ten minutes per account).
	 */
	async published(accounts: string[]): Promise<DjSetView[] | null> {
		if (accounts.length === 0) return [];
		await Promise.all(accounts.map((did) => this.backfill(did)));

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

	private async backfill(did: string): Promise<void> {
		const last = this.backfilled.get(did);

		if (!this.directory.admin || (last !== undefined && Date.now() - last < BACKFILL_EVERY_MS)) return;
		this.backfilled.set(did, Date.now());

		try {
			const job = await this.directory.backfill(COLLECTION.djSet, did);
			const status = await this.directory.awaitBackfill(job, BACKFILL_WAIT_MS);

			if (status !== 'completed') this.core.logger.info({ did, status }, 'HappyView is still reading sets');
		} catch (error) {
			// The picker still lists what HappyView has, and the next open tries again.
			this.backfilled.delete(did);
			this.core.logger.warn({ err: error, did }, 'could not have HappyView read sets');
		}
	}
}
