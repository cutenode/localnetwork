// The offscreen page: a document with nothing on it, which the background
// worker opens for what only an extension page can do, and closes again once
// nothing is waiting (background.ts says why it cannot do these itself).
//
// It is an extension page like the popup, so its fetches go out with the
// extension's host permissions and it reads the popup's localStorage, where
// the address, the name and the mark setting live ($lib/downloader.svelte.ts).
// What it cannot do is open a tab (or use any extension API but messaging), so
// the one step that needs one, a Spotify sign-in, goes back to the worker.
//
// Its errands: walking a gate for the page's Download button, and reading the
// settings a gate done on its own page uses.

import { downloader } from '$lib/downloader.svelte.ts';
import type { GatePagePrefs, OffscreenRequest, RoundTripRequest, WalkGateAnswer } from '$lib/types';
import { openUrlOf, siteFor } from './tools/third-party-downloads/sites/index.ts';
import { walk as walkGate } from './tools/third-party-downloads/walks/index.ts';

const connect = (url: string, { landing }: { landing: string }): Promise<boolean> =>
	chrome.runtime.sendMessage({ type: 'ROUND_TRIP', url, landing } satisfies RoundTripRequest);

/**
 * One gate, walked the way the Downloader panel walks a row. Only the walked sites are walked
 * from the page: the others are done on their own page, or only opened, and opening the link is
 * already the button's answer for those.
 */
async function walk(url: string): Promise<WalkGateAnswer> {
	const site = siteFor(url);

	if (site?.how !== 'walk') return { ok: false, openUrl: openUrlOf(url) };

	try {
		const result = await walkGate(site, url, {
			email: downloader.email.value.trim(),
			name: downloader.name.value.trim(),
			connect
		});

		return {
			ok: true,
			url: result.url,
			subfolder: site.subfolder,
			mark: downloader.mark.value
		};
	} catch (_e) {
		// Whatever the gate wanted, it is the person's to give it now: the gate opens.
		return { ok: false, openUrl: url };
	}
}

chrome.runtime.onMessage.addListener((message: OffscreenRequest, _sender, sendResponse) => {
	switch (message?.type) {
		case 'WALK_GATE':
			void walk(message.url).then(sendResponse);

			return true;

		case 'READ_PREFS':
			sendResponse({ email: downloader.email.value.trim(), mark: downloader.mark.value } satisfies GatePagePrefs);

			return undefined;

		default:
			return undefined;
	}
});
