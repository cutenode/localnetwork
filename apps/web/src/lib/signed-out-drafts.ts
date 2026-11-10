/**
 * Clearing the drafts of accounts that signed out (see form-draft.svelte.ts). It's apart from the drafts themselves
 * because every page runs it, while only the pages with long forms keep drafts.
 */

/** What every draft's storage key starts with, before the account's DID. */
export const DRAFT_PREFIX = 'ln:draft:';

/**
 * Names the accounts that signed out in this browser since it last ran the app's scripts, set by the sign-out
 * response itself so it doesn't matter whether the page signed out from had loaded them.
 */
export const SIGNED_OUT_COOKIE = 'ln_signed_out';

/** Drops every draft kept in this browser by the accounts that signed out in it, then the cookie naming them. */
export function clearSignedOutDrafts() {
	const value = document.cookie.match(new RegExp(`(?:^|; )${SIGNED_OUT_COOKIE}=([^;]*)`))?.[1];

	if (!value) return;
	const dids = decodeURIComponent(value).split(' ');

	try {
		for (const key of Object.keys(localStorage))
			if (dids.some((did) => key.startsWith(`${DRAFT_PREFIX}${did}:`))) localStorage.removeItem(key);
	} catch {
		// Blocked storage holds no drafts.
	}

	document.cookie = `${SIGNED_OUT_COOKIE}=; Max-Age=0; Path=/`;
}
