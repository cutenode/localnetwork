import type { Selectable } from 'kysely';
import { COLLECTION } from '#lib/nsid.ts';
import type { BasicsSection, PartyRecord, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { type Core, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import type { PartyTable } from '../db/schema';
import { savedChanges } from './diff';
import { PARTY_KEY, partyRecord } from './party-index';
import { type PageInput, pageAvatar, type SharedSpaces, STALE_PAGE } from './shared-spaces';

/**
 * What a party's page form writes: its basics, what happens to its avatar, and whether this save publishes it. A draft
 * stays one until a save publishes it; publishing a published party changes nothing.
 */
export type PartyInput = Pick<PageInput, 'avatar'> & { basics: Omit<BasicsSection, 'avatar'>; publish: boolean };

/** Saving a party's page: its form, and the version of the page the form was opened on. */
export type PartyPageInput = PartyInput & Pick<PageInput, 'swap'>;

/** A party as the site shows it. `publishedAt` is null while it's a draft, which only its people and staff see. */
export type PartyView = {
	space: string;
	skey: string;
	basics: BasicsSection;
	avatarUrl: string | null;
	publishedAt: string | null;
	cid: string;
};

/**
 * Parties, each in a party space under the root that holds its page as the root's party record. A party goes by its
 * space's key, and its name can repeat (ADR 0016). Starting one makes its creator its host, and people with
 * page.edit, or staff, save its page. A party starts as a draft unless it's published as it starts.
 */
export class PartyService {
	constructor(
		private readonly core: Core,
		private readonly sharedSpaces: SharedSpaces
	) {}

	/** Starts a party with `actor` as its host, and returns its space. */
	async start(actor: Viewer, input: PartyInput): Promise<string> {
		const root = await this.core.root();
		const avatar = await pageAvatar(root, input.avatar, undefined);
		const now = nowIso();

		const record = validateRecord(COLLECTION.party, {
			basics: avatar ? { ...input.basics, avatar } : input.basics,
			...(input.publish && { publishedAt: now }),
			createdAt: now,
			updatedAt: now
		});

		const space = await this.sharedSpaces.create(actor, 'party', {
			writes: [{ action: 'create', collection: COLLECTION.party, rkey: PARTY_KEY, record }],
			summary: `Started ${input.basics.name}${input.publish ? '' : ' as a draft'}`
		});

		this.core.logger.info('started a party', { space, published: input.publish });

		return space;
	}

	/** Saves a party's page, and publishes it when asked. Needs page.edit. */
	async updatePage(actor: Viewer, space: string, input: PartyPageInput): Promise<void> {
		await this.sharedSpaces.change(space, async (live) => {
			const actingAs = this.sharedSpaces.authorize(actor, live, 'page.edit');
			const ref = { space, repo: live.root.did, collection: COLLECTION.party, rkey: PARTY_KEY };
			const stored = await live.root.getSpaceRecord(ref);
			const current = partyRecord(stored?.value);

			if (!stored || !current) throw new NotFoundError('That party does not exist');

			if (stored.cid !== input.swap) throw new UserError(STALE_PAGE, 409);
			const avatar = await pageAvatar(live.root, input.avatar, current.basics.avatar);
			const now = nowIso();
			const publishes = input.publish && !current.publishedAt;

			const record = validateRecord(COLLECTION.party, {
				...current,
				basics: avatar ? { ...input.basics, avatar } : input.basics,
				...(publishes && { publishedAt: now }),
				updatedAt: now
			});

			await live.root.putSpaceRecord({ space, collection: COLLECTION.party, rkey: PARTY_KEY, record });
			await this.sharedSpaces.index({ space, collection: COLLECTION.party, rkey: PARTY_KEY, value: record });

			const { name } = input.basics;

			await this.sharedSpaces.record(
				actor,
				actingAs,
				'page.update',
				space,
				publishes ? `Published ${name}` : `Updated ${name}'s page`,
				{ page: 'party' },
				savedChanges(current, record)
			);
		});
	}

	async bySkey(skey: string): Promise<PartyView | null> {
		return this.first('skey', skey);
	}

	async bySpace(space: string): Promise<PartyView | null> {
		return this.first('spaceUri', space);
	}

	private async first(column: 'skey' | 'spaceUri', value: string): Promise<PartyView | null> {
		const row = await this.core.db.selectFrom('party').selectAll().where(column, '=', value).executeTakeFirst();

		return row ? this.toView(row) : null;
	}

	private toView(row: Selectable<PartyTable>): PartyView {
		// SAFETY: indexParty is the only writer of the party table, and stores a record valid by the party lexicon.
		const { basics } = JSON.parse(row.record) as PartyRecord;
		const avatar = basics.avatar?.ref.$link;

		return {
			space: row.spaceUri,
			skey: row.skey,
			basics,
			avatarUrl: avatar ? `/media/${this.core.rootDid}/${avatar}` : null,
			publishedAt: row.publishedAt,
			cid: row.cid
		};
	}
}
