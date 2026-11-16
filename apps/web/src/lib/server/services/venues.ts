import { type Selectable, type SqlBool, sql } from 'kysely';
import { COLLECTION } from '#lib/nsid.ts';
import type { VenueApproval, VenueRecord, Viewer } from '#lib/types.ts';
import type { VenueOption } from '#lib/venues.ts';
import { validateRecord } from '../atproto/lexicons';
import { type Core, UserError } from '../core';
import { nowIso } from '../db';
import type { VenueTable } from '../db/schema';
import type { RateLimiter } from '../rate-limit';
import type { SharedSpaces } from './shared-spaces';
import { nameKey, VENUE_KEY } from './venue-index';

/** What adding a venue takes: its name and where it is, with its Foursquare place when known. */
export type VenueInput = Pick<VenueRecord, 'name' | 'address' | 'geo' | 'fsq'>;

export type VenueView = {
	space: string;
	skey: string;
	name: string;
	address: VenueRecord['address'];
	geo?: VenueRecord['geo'];
	fsqPlaceId?: string;
	approval: VenueApproval;
	cid: string;
};

/** How many venues a search suggests. */
const SEARCH_LIMIT = 8;

/** `text` as a LIKE pattern that matches it literally, with `\` as the escape. */
const likeLiteral = (text: string) => text.replace(/[\\%_]/g, (char) => `\\${char}`);

/**
 * Venues, one per place, each in a venue space under the root. Adding one puts nobody in it: owning a venue takes a
 * confirmed claim. Every venue is listed from the start, since it's a public place, and staff can unlist one.
 */
export class VenueService {
	constructor(
		private readonly core: Core,
		private readonly sharedSpaces: SharedSpaces,
		/** Per account, on top of the per-address limit every form post has. */
		private readonly limiter: RateLimiter
	) {}

	/**
	 * Adds a venue, or returns the one that already has its Foursquare place, else the one with the same name in the
	 * same city and country. Names repeat (ADR 0016), so that's only a guess at the same place, and a venue with another
	 * Foursquare place is never it.
	 */
	async add(actor: Viewer, input: VenueInput): Promise<VenueView> {
		if (!this.limiter.take(`venues:${actor.did}`))
			throw new UserError('Too many requests. Try again in a minute.', 429);
		const name = input.name.trim();

		if (!name) throw new UserError("Enter the venue's name");

		// One add at a time, so two can't both find the place free.
		return this.sharedSpaces.locked('venues', async () => {
			const placeId = input.fsq?.fsq_place_id;

			const existing =
				(placeId && (await this.byPlaceId(placeId))) || (await this.byNameIn(name, input.address, placeId));

			if (existing) {
				this.core.logger.info('gave back an existing venue', { space: existing.space, name });

				return existing;
			}

			const now = nowIso();

			const record = validateRecord(COLLECTION.venue, {
				name,
				address: input.address,
				...(input.geo && { geo: input.geo }),
				...(input.fsq && { fsq: input.fsq }),
				approval: 'open',
				createdAt: now,
				updatedAt: now
			});

			const space = await this.sharedSpaces.create(actor, 'venue', {
				writes: [{ action: 'create', collection: COLLECTION.venue, rkey: VENUE_KEY, record }],
				summary: `Added ${name}`,
				joins: false,
				listed: true
			});

			const added = await this.bySpace(space);

			// SAFETY: create indexed the venue record it wrote.
			return added!;
		});
	}

	async bySkey(skey: string): Promise<VenueView | null> {
		return this.first('skey', skey);
	}

	async byPlaceId(placeId: string): Promise<VenueView | null> {
		return this.first('fsqPlaceId', placeId);
	}

	async bySpace(space: string): Promise<VenueView | null> {
		return this.first('spaceUri', space);
	}

	/** Venues whose name has `query` in it, shortest name first, so an exact match leads. */
	async search(query: string): Promise<VenueOption[]> {
		const text = nameKey(query);

		if (text.length < 2) return [];

		const rows = await this.core.db
			.selectFrom('venue')
			.select(['spaceUri', 'skey', 'name', 'locality', 'country'])
			.where(sql<SqlBool>`name_key like ${`%${likeLiteral(text)}%`} escape '\\'`)
			.orderBy(sql`length(name)`)
			.orderBy('name')
			.limit(SEARCH_LIMIT)
			.execute();

		return rows.map(({ spaceUri, skey, name, locality, country }) => ({
			space: spaceUri,
			skey,
			name,
			...(locality && { locality }),
			country
		}));
	}

	/**
	 * The venue with this name, typed in any case or spacing, in the same city and country. With `placeId`, only one
	 * whose Foursquare place isn't known yet, since one with another place is somewhere else.
	 */
	private async byNameIn(
		name: string,
		address: VenueRecord['address'],
		placeId: string | undefined
	): Promise<VenueView | null> {
		const row = await this.core.db
			.selectFrom('venue')
			.selectAll()
			.where('nameKey', '=', nameKey(name))
			.where('locality', 'is', address.locality ?? null)
			.where('country', '=', address.country)
			.$if(!!placeId, (query) => query.where('fsqPlaceId', 'is', null))
			.executeTakeFirst();

		return row ? toView(row) : null;
	}

	private async first(column: 'skey' | 'fsqPlaceId' | 'spaceUri', value: string): Promise<VenueView | null> {
		const row = await this.core.db.selectFrom('venue').selectAll().where(column, '=', value).executeTakeFirst();

		return row ? toView(row) : null;
	}
}

/** A venue as the venue picker offers it. */
export function venueOption({ space, skey, name, address }: VenueView): VenueOption {
	return { space, skey, name, ...(address.locality && { locality: address.locality }), country: address.country };
}

function toView(row: Selectable<VenueTable>): VenueView {
	// SAFETY: indexVenue is the only writer of the venue table, and stores a record valid by the venue lexicon.
	const record = JSON.parse(row.record) as VenueRecord;

	return {
		space: row.spaceUri,
		skey: row.skey,
		name: row.name,
		address: record.address,
		...(record.geo && { geo: record.geo }),
		...(row.fsqPlaceId && { fsqPlaceId: row.fsqPlaceId }),
		approval: record.approval === 'approval' ? 'approval' : 'open',
		cid: row.cid
	};
}
