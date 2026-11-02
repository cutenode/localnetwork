import { createHash } from 'node:crypto';
import type { JsonObject } from '@atproto/lex-json';
import { isListedRole, SPACE_KINDS, type SpaceKind } from '#lib/capabilities.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { validateRecord } from '../atproto/lexicons';
import { type StoredRecord, xrpcErrorName } from '../atproto/repo-client';
import { parseSpaceUri, sharedSpace } from '../atproto/spaces';
import { canonicalJson } from '../audit/chain';
import type { Core } from '../core';
import { nowIso } from '../db';

/** What a listing shows of its space beyond its people, from the kind's {@link ListingSource}. */
export type ListingFields = { name: string; slug?: string; fields?: JsonObject };

/** Reads what a kind's public page shows, or null when there's nothing to list yet. */
export type ListingSource = { publicFields(space: string): Promise<ListingFields | null> };

/** The listing's kind for each kind of space: a listed performer is an act, since a DJ under their own name has a profile. */
const LISTING_KIND = { performer: 'act', party: 'party', venue: 'venue' } as const satisfies Record<SpaceKind, string>;

/** At most this many people go in a listing, as its lexicon allows. */
const MAX_PEOPLE = 50;

const digestKey = (space: string) => `listing_digest:${space}`;

const rkeyOf = (record: StoredRecord) => record.uri.split('/').at(-1) ?? '';

const digestOf = (record: JsonObject) => createHash('sha256').update(canonicalJson(record)).digest('base64url');

/** A published listing's digest, leaving out what changes on every write. */
const publishedDigest = ({ value }: StoredRecord) => {
	const { $type: _type, updatedAt: _updatedAt, ...record } = value;

	return digestOf(record);
};

/**
 * Writes each listed act's, party's or venue's public listing to the root's repo, from its space, so HappyView can
 * index what spaces keep from it. A listing holds only what the public page shows and the people in a listed role who
 * haven't hidden; it never holds capabilities, how anyone came in, invites or requests. It's rewritten only when it
 * would change, and deleted when the space isn't listed or its kind has nothing to list.
 */
export class ListingWriter {
	/** The latest refresh of each space, and one that hasn't started yet, which later calls share. */
	private readonly runs = new Map<string, Promise<void>>();
	private readonly waiting = new Map<string, Promise<void>>();

	constructor(
		private readonly core: Core,
		private readonly sources: Partial<Record<SpaceKind, ListingSource>>
	) {}

	/**
	 * Brings the space's listing in line with the space. Calls while one runs share the next run, so a burst of synced
	 * records writes once. A failure is logged; the hourly reconcile tries again.
	 */
	refresh(space: string): Promise<void> {
		const waiting = this.waiting.get(space);

		if (waiting) return waiting;
		const before = this.runs.get(space);

		const run = (async () => {
			await before;
			this.waiting.delete(space);

			try {
				await this.update(space);
			} catch (error) {
				this.core.logger.warn({ err: error, space }, "could not update a space's listing");
			}
		})();

		this.waiting.set(space, run);
		this.runs.set(space, run);
		void run.finally(() => {
			if (this.runs.get(space) === run) this.runs.delete(space);
		});

		return run;
	}

	/**
	 * Checks every listing against the root's repo (run at startup and every hour): rewrites the ones that differ,
	 * deletes the ones whose space isn't listed, and deletes listings whose space is gone.
	 */
	async reconcile(): Promise<void> {
		const root = await this.core.root();
		// The root's own repo: each listing is checked against what the root wrote, which a directory that's behind can't say.
		// ast-grep-ignore: no-direct-pds-read
		const records = await root.listRecords(COLLECTION.listing);
		const published = new Map(records.map((record) => [rkeyOf(record), record]));
		const spaces = await this.core.db.selectFrom('sharedSpace').select('spaceUri').execute();

		for (const { spaceUri } of spaces) {
			const skey = parseSpaceUri(spaceUri)?.skey ?? '';

			try {
				await this.update(spaceUri, published.get(skey) ?? null);
			} catch (error) {
				this.core.logger.warn({ err: error, space: spaceUri }, "could not check a space's listing");
			}

			published.delete(skey);
		}

		// Listings of spaces the index doesn't know: kept while the space is there, since the index may be catching up.
		for (const rkey of published.keys()) {
			const exists = await Promise.all(SPACE_KINDS.map((kind) => root.getSpace(sharedSpace(root.did, kind, rkey))));

			if (exists.some(Boolean)) continue;
			await root.deleteRecord({ collection: COLLECTION.listing, rkey });
			this.core.logger.info({ rkey }, 'deleted the listing of a space that is gone');
		}
	}

	/** Writes or deletes the space's listing. `published` is the listing as the root's repo has it, when known. */
	private async update(space: string, published?: StoredRecord | null): Promise<void> {
		const { db } = this.core;
		const rkey = parseSpaceUri(space)?.skey;

		if (!rkey) return;

		const status = await db
			.selectFrom('sharedSpace')
			.select(['kind', 'listed'])
			.where('spaceUri', '=', space)
			.executeTakeFirst();

		const source = status && this.sources[status.kind];
		const fields = status?.listed === 1 && source ? await source.publicFields(space) : null;
		const stored = await db.selectFrom('kv').select('value').where('key', '=', digestKey(space)).executeTakeFirst();
		const root = await this.core.root();

		const current = () => (published === undefined ? this.published(rkey) : published);

		if (!status || !fields) {
			// Nothing was written since the last check, so there's nothing to delete.
			if (published === undefined && !stored) return;

			if (await current()) {
				await root.deleteRecord({ collection: COLLECTION.listing, rkey });
				this.core.logger.info({ space }, 'deleted a listing');
			}

			await db.deleteFrom('kv').where('key', '=', digestKey(space)).execute();

			return;
		}

		const record = await this.build(space, status.kind, fields);

		if (!record) return;
		const digest = digestOf(record);

		if (published === undefined && stored?.value === digest) return;
		const before = await current();

		if (!before || publishedDigest(before) !== digest) await this.write(space, rkey, record, before);

		await db
			.insertInto('kv')
			.values({ key: digestKey(space), value: digest })
			.onConflict((oc) => oc.column('key').doUpdateSet({ value: digest }))
			.execute();
	}

	/** The listing, or null when it would name someone hidden, which is never written. */
	private async build(space: string, kind: SpaceKind, fields: ListingFields): Promise<JsonObject | null> {
		const rows = await this.core.db
			.selectFrom('spacePerson')
			.select(['did', 'role', 'hidden'])
			.where('spaceUri', '=', space)
			// Oldest first; by DID among people added together, so the same people always read the same.
			.orderBy('createdAt')
			.orderBy('did')
			.execute();

		const people = rows
			.filter((row) => row.hidden === 0 && isListedRole(kind, row.role))
			.slice(0, MAX_PEOPLE)
			.map(({ did, role }) => ({ did, role }));

		// Round-tripped, so a field left undefined is gone before the digest and the safety check read it.
		const record: JsonObject = JSON.parse(JSON.stringify({ kind: LISTING_KIND[kind], ...fields, people }));
		const json = JSON.stringify(record);
		const leaked = rows.filter((row) => row.hidden === 1 && json.includes(row.did));

		if (leaked.length > 0) {
			this.core.logger.error({ space }, 'refusing to write a listing that names someone hidden');

			return null;
		}

		return record;
	}

	/** The listing as the root's own repo has it now, which is what each write swaps out. */
	private async published(rkey: string): Promise<StoredRecord | null> {
		const root = await this.core.root();

		// The root's own repo, since a write has to swap the record that's there now.
		// ast-grep-ignore: no-direct-pds-read
		return root.getRecord({ collection: COLLECTION.listing, rkey });
	}

	/** Writes over exactly the listing that was read, reading it again once if it changed in between. */
	private async write(space: string, rkey: string, record: JsonObject, before: StoredRecord | null): Promise<void> {
		const root = await this.core.root();
		const value = validateRecord(COLLECTION.listing, { ...record, updatedAt: nowIso() });

		const put = (swap: StoredRecord | null) =>
			root.putRecord({ collection: COLLECTION.listing, rkey, record: value, swapRecord: swap?.cid ?? null });

		try {
			await put(before);
		} catch (error) {
			if (xrpcErrorName(error) !== 'InvalidSwap') throw error;
			await put(await this.published(rkey));
		}

		this.core.logger.info({ space }, 'wrote a listing');
	}
}
