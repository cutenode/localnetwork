// Asking a SoundCloud tab about artists: who is behind each path, and what
// links they put on their profile.
//
// The requests are the content script's to make (content.ts,
// GET_ARTIST_PROFILES), for the reason every SoundCloud request is: the API
// takes them from the page's origin, with the page's own client id, and from
// nowhere else. So a lookup needs a SoundCloud tab in front — any page of it,
// not a playlist — and this is what asks it, and what explains instead when
// it cannot.

import { activeTab, askPage, isSoundCloudUrl } from '$lib/chrome.ts';
import { RELOAD_THE_TAB } from '$lib/soundcloud.ts';
import type { PageRead, ProfilesAnswer } from '$lib/types';

export const OPEN_A_TAB = 'open a SoundCloud tab to look up socials.';

export const LOOKUP_FAILED = 'couldn’t look them up. reload the SoundCloud tab and try again.';

/** Whether the tab in front is a SoundCloud page a lookup can be asked of. */
export async function onSoundCloud() {
	const tab = await activeTab();

	return !!tab && isSoundCloudUrl(tab.url);
}

/**
 * Asks the active tab's content script for the profiles behind `permalinks`.
 *
 * Progress arrives separately, as ARTIST_PROFILES_PROGRESS messages on
 * chrome.runtime.onMessage, while this waits for the whole answer.
 *
 * @param permalinks artists' paths
 * @returns the page's answer — `profiles`, `failed`, `error` — or why there is none.
 */
export async function lookupProfiles(permalinks: string[]): Promise<PageRead<ProfilesAnswer>> {
	const tab = await activeTab();

	if (!tab || !isSoundCloudUrl(tab.url)) return { status: OPEN_A_TAB };
	const { reached, response } = await askPage(tab.id!, 'GET_ARTIST_PROFILES', { permalinks });

	if (!reached) return { status: RELOAD_THE_TAB };

	// A content script from before this message existed answers nothing at
	// all, and one that failed answers `ok: false`; either is a tab to reload.
	if (!response || !response.ok || !Array.isArray(response.profiles)) {
		return { status: LOOKUP_FAILED };
	}

	return { data: response };
}
