// What the content script says stopped a lookup short, in words for a panel.
// Manager's download details and the artists' socials are both asked of
// SoundCloud's API from the page, so they fail the same two ways.

import type { LookupError } from '$lib/types';

/** '' is no failure, and has no sentence. */
export const LOOKUP_ERRORS: Partial<Record<LookupError, string>> = {
	'no-client-id': 'couldn’t find SoundCloud’s API key in the page. reload the tab.',
	unauthorized: 'SoundCloud rejected the lookups (the page’s API key has expired). reload the tab.'
};
