const DECIDED = new Map(
	Object.entries({
		approved: 'Approved and published',
		listed: 'Approved and listed in Find DJs',
		rejected: 'Rejected',
		changesRequested: 'Sent back for changes'
	})
);

/** What a decision did, from the `decided` parameter deciding redirects with to the next request or the queue. */
export const decidedNotice = (outcome: string | null) => (outcome && DECIDED.get(outcome)) || null;
