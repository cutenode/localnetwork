// Taking the profiles an older version kept off the stored artists list.
//
// Versions before this one kept what SoundCloud said about each artist (bio,
// city, followers, links) on the list in chrome.storage.local, with no end
// date. That is another person's data, now held for the browsing session only
// (artists.svelte.ts says why). The background worker runs this when the
// extension is installed over such a version, so it leaves the disk without
// waiting for the artists panel to open. The panel fills the profiles in again
// from the next SoundCloud tab it is opened on.
//
// Plain TypeScript rather than part of the store, so the worker can run it
// without the popup's runtime.

/** artists.svelte.ts's ARTISTS_KEY; test/consistency.test.ts keeps them in step. */
const KEY = 'artists';

const carries = (entry: unknown): entry is { profile: unknown } =>
	entry instanceof Object && 'profile' in entry && entry.profile !== null;

/** Writes the stored list back with `profile: null` on every entry that had one, if any did. */
export async function dropOldProfiles() {
	const list: unknown = (await chrome.storage.local.get(KEY))[KEY];

	if (!Array.isArray(list) || !list.some(carries)) return;

	await chrome.storage.local.set({
		[KEY]: list.map((entry) => (carries(entry) ? { ...entry, profile: null } : entry))
	});
}
