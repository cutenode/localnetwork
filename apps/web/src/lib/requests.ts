import { CLAIM_RELATIONSHIPS, VENUE_CLAIM_RELATIONSHIPS } from '#lib/taxonomy/options.ts';
import type { RequestRecord } from '#lib/types.ts';

const DECIDED = new Map(
	Object.entries({
		approved: 'Approved and published',
		listed: 'Approved and listed in Find DJs',
		owner: 'Approved and added as an owner',
		rejected: 'Rejected',
		changesRequested: 'Sent back for changes'
	})
);

/** What a decision did, from the `decided` parameter deciding redirects with to the next request or the queue. */
export const decidedNotice = (outcome: string | null) => (outcome && DECIDED.get(outcome)) || null;

/** What a claim asks about: a venue, which only an owner claims, or a project. */
export const claimTerms = (record: Pick<RequestRecord, 'space'>) =>
	record.space
		? {
				relationships: VENUE_CLAIM_RELATIONSHIPS,
				noun: 'venue',
				example: 'For example: our website lists me as its owner.'
			}
		: {
				relationships: CLAIM_RELATIONSHIPS,
				noun: 'project',
				example: 'For example: I added it to our SoundCloud bio.'
			};
