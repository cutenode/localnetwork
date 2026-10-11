// Getting a SoundCloud page's tracks, and saying why there are none.
//
// All three tools open the same way: find the active tab, check it is
// a SoundCloud page, ask the content script for what is on it, and — far more
// often than one would like — explain instead why that could not be done. The
// wording of those explanations used to be copied into each panel; it lives
// here now, so a page that stops loading its likes says the same thing
// whichever tool noticed.

import type { DetailsAnswer, PageRead, TracksAnswer } from '$lib/types';
import { activeTab, askPage, isSoundCloudUrl } from './chrome.ts';

/**
 * What `readPage` hands over for each question it asks: the answer, never the failure, which it
 * turns into a status instead.
 */
export interface PageReads {
	EXTRACT_TRACKS: TracksAnswer;
	GET_DOWNLOAD_DETAILS: DetailsAnswer;
}

export interface ReadOptions {
	allowEmpty?: boolean;
}

const OPEN_A_PAGE = 'open a SoundCloud playlist, likes or track page in this tab.';

const RELOAD_THE_TAB = 'can’t reach the page — reload the tab.';

const EXTRACTION_FAILED = 'couldn’t read the page. reload the tab and try again.';

// Why a page the content script *did* answer for has no tracks to show,
// depending on what kind of page it saw (`pageType`: 'playlist', 'likes',
// 'track' or 'other').
export function emptyStatus(response: TracksAnswer) {
	if (response.pageType === 'playlist') {
		return 'no tracks here. let the playlist finish loading.';
	}

	if (response.pageType === 'likes') {
		return response.skippedPlaylists > 0
			? `only liked playlists here (${response.skippedPlaylists} skipped). scroll down to load liked tracks.`
			: 'no tracks here. let the likes page finish loading.';
	}

	// A track page names its track in the hero and nowhere else, so a hero that
	// has not rendered yet is the whole of what went wrong here.
	if (response.pageType === 'track') {
		return 'no track here yet. let the page finish loading.';
	}

	return 'that’s not a playlist, likes or track page.';
}

// Reads started before the panel that wants them exists. Reading a page is
// two round trips — the browser process for the tab, then the content script,
// which for download details is a good many requests to SoundCloud — and none
// of them need wait for a chunk to compile or a component to mount. The shell
// starts them at boot for the tool it is about to open (see $lib/tools.js), so
// by the time the panel asks, the answer is on its way or already here.
//
// One per type, because the Manager tool asks for two: the tracks, which
// come back in a few milliseconds and are what the list is drawn from, and
// their download details, which take as long as SoundCloud takes. Both are
// started at boot and neither waits for the other.
const warmed = new Map<keyof PageReads, Promise<PageRead<TracksAnswer | DetailsAnswer>>>();

/**
 * Starts `readPage(type)` now, for the panel that will ask for it next.
 *
 * One read per type, handed to the first matching `readPage`; a second ask
 * for the same type goes to the page as it always did. Warming a type twice
 * — the shell settling on the tool it was already opening — starts nothing
 * new.
 */
export function warmPage(type: keyof PageReads, options?: ReadOptions) {
	if (warmed.has(type)) return;
	warmed.set(type, readPage(type, options));
}

/** What a caller sees: the answer typed by the question asked. */
export function readPage<T extends keyof PageReads>(type: T, options?: ReadOptions): Promise<PageRead<PageReads[T]>>;
/**
 * Asks the active tab's content script for its tracks.
 *
 * @param type what to ask for: the tracks alone, or the tracks with their download details.
 * @param options `allowEmpty` keeps a page with no tracks from being turned into a status
 *   message. No caller passes it today (the Downloader asks the page itself), and a warmed
 *   read keeps the options it was warmed with, whatever these say.
 * @returns the page's answer, or the sentence explaining why there is none.
 */
export async function readPage(
	type: keyof PageReads,
	{ allowEmpty = false }: ReadOptions = {}
): Promise<PageRead<TracksAnswer | DetailsAnswer>> {
	if (warmed.has(type)) {
		const read = warmed.get(type)!;
		warmed.delete(type);

		return read;
	}

	const tab = await activeTab();

	if (!tab || !isSoundCloudUrl(tab.url)) return { status: OPEN_A_PAGE };

	const { reached, response } = await askPage(tab.id!, type);

	if (!reached || !response) return { status: reached ? EXTRACTION_FAILED : RELOAD_THE_TAB };

	// `ok` alone is not enough. A tab open across an extension update still runs
	// the content script it was loaded with, and that one answers in the shape
	// *it* knows — which a later version cannot assume carries any tracks at
	// all. Anything that is not this version's answer is a page to reload.
	if (!response.ok || !Array.isArray(response.tracks)) return { status: EXTRACTION_FAILED };

	if (!allowEmpty && response.tracks.length === 0) return { status: emptyStatus(response) };

	return { data: response };
}

// The line under a panel's title: what the page is, and how much of it there
// is. `extra` is whatever the tool wants to add (its own count of links, say).
export function pageSummary({ title, curator, tracks, skippedPlaylists }: TracksAnswer, extra: string[] = []) {
	const parts = [];

	if (title) parts.push(title);

	if (curator) parts.push(`by ${curator}`);
	parts.push(count(tracks.length, 'track'));
	parts.push(...extra);

	// Likes pages can mix liked tracks with liked playlists; the content script
	// skips the playlists, so say so rather than silently under-counting.
	if (skippedPlaylists > 0) parts.push(`${count(skippedPlaylists, 'liked playlist')} skipped`);

	return parts.join(' · ');
}

/** "1 track", "3 tracks" — the plural is the singular plus an s unless given. */
export const count = (n: number, singular: string, plural = `${singular}s`) => `${n} ${n === 1 ? singular : plural}`;

export { OPEN_A_PAGE, RELOAD_THE_TAB, EXTRACTION_FAILED };
