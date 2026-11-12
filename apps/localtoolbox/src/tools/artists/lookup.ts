// Asking a SoundCloud tab about artists: who is behind each path, and what
// links they put on their profile.
//
// The content script answers (content.ts, GET_ARTIST_PROFILES) from what its
// page has loaded, which is the only place that is known. So filling the list
// in needs a SoundCloud tab in front (any page of it, not just a playlist),
// and this is what asks it, and what explains instead when it cannot.

import { activeTab, askPage, isSoundCloudUrl } from '$lib/chrome.ts';
import { RELOAD_THE_TAB } from '$lib/soundcloud.ts';
import type { PageRead, ProfilesAnswer } from '$lib/types';

export const OPEN_A_TAB = 'open a SoundCloud tab to fill in socials.';

export const LOOKUP_FAILED = 'couldn’t read them. reload the SoundCloud tab and try again.';

/** Whether the tab in front is a SoundCloud page a lookup can be asked of. */
export async function onSoundCloud() {
	const tab = await activeTab();

	return !!tab && isSoundCloudUrl(tab.url);
}

/**
 * Asks the active tab's content script for the profiles behind `permalinks`.
 *
 * @param permalinks artists' paths
 * @returns the profiles the page has loaded of those, or why there is no answer.
 */
export async function lookupProfiles(permalinks: string[]): Promise<PageRead<ProfilesAnswer>> {
	const tab = await activeTab();

	if (!tab || !isSoundCloudUrl(tab.url)) return { status: OPEN_A_TAB };
	const { reached, response } = await askPage(tab.id!, 'GET_ARTIST_PROFILES', { permalinks });

	if (!reached) return { status: RELOAD_THE_TAB };

	// A content script from before this answer's shape answers in its own, or
	// not at all; either is a tab to reload.
	if (!response || !response.ok || !Array.isArray(response.profiles)) {
		return { status: LOOKUP_FAILED };
	}

	return { data: response };
}
