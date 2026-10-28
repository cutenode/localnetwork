// The offscreen page: a document with nothing on it, which the background
// worker opens to walk a download gate and closes again once nothing is
// waiting (background.ts says why it cannot walk one itself).
//
// It is an extension page like the popup, so its fetches go out with the
// extension's host permissions and it reads the popup's localStorage, where
// the address, the name and the mark setting live
// ($lib/downloader.svelte.ts). What it cannot do is open a tab, so the one
// step that needs one, a Spotify sign-in, goes back to the worker.

import { downloader } from '$lib/downloader.svelte.ts';
import type { RoundTripRequest, WalkGateAnswer, WalkGateRequest } from '$lib/types';
import { siteFor } from './tools/third-party-downloads/sites/index.ts';

const connect = (url: string, { landing }: { landing: string }): Promise<boolean> =>
	chrome.runtime.sendMessage({ type: 'ROUND_TRIP', url, landing } satisfies RoundTripRequest);

/**
 * One gate, walked the way the Downloader panel walks a row. Only the sites whose walk ends in
 * a file are walked from the page: the others end in a page to open or a signup, and opening
 * the link is already the button's answer for those.
 */
async function walk(url: string): Promise<WalkGateAnswer> {
	const site = siteFor(url);

	if (site?.yields !== 'file') return { ok: false, openUrl: url };

	try {
		const result = await site.unlock(url, {
			email: downloader.email.value.trim(),
			name: downloader.name.value.trim(),
			connect
		});

		if (result.kind !== 'file') return { ok: false, openUrl: result.url };

		return {
			ok: true,
			url: result.url,
			filename: result.filename ?? '',
			subfolder: site.subfolder,
			mark: downloader.mark.value
		};
	} catch (_e) {
		// Whatever the gate wanted, it is the person's to give it now: the gate opens.
		return { ok: false, openUrl: url };
	}
}

chrome.runtime.onMessage.addListener((message: WalkGateRequest, _sender, sendResponse) => {
	if (message?.type !== 'WALK_GATE') return undefined;
	void walk(message.url).then(sendResponse);

	return true;
});
