// The background worker, for the one thing the page cannot do by itself: walk
// a download gate when the Download button the content script put on the page
// is pressed on a link to one (content.ts, "Replacing SoundCloud's own
// download buttons").
//
// The walk is the Downloader panel's own, site module and all, and neither of
// the two obvious places can run it. A content script fetches as the page it
// is in, so every gate is another origin it is refused; a service worker has
// no DOMParser, which the Hypeddit and Pump Your Sound walks read their pages
// with. So the walk runs in an offscreen page (offscreen.ts), a document
// Chrome opens for exactly this, and this worker does the parts only it can:
// the tab a Spotify step is walked in, the download, and the tab the gate
// opens in when the walk ends anywhere but a file.

import { roundTrip } from '$lib/chrome.ts';
import type { RoundTripRequest, SaveGateAnswer, SaveGateRequest, WalkGateAnswer, WalkGateRequest } from '$lib/types';
import { save } from './tools/third-party-downloads/save.ts';

const OFFSCREEN = 'offscreen.html';

// One walk at a time, for the panel's reason: several of these sites hand out
// a per-visit token, and every one of them is somebody's small server. The
// offscreen page is closed once nothing is waiting, so each burst of clicks
// reads the settings afresh and an idle browser holds no document for it.
let turn: Promise<unknown> = Promise.resolve();

let waiting = 0;

async function openOffscreen() {
	const open = await chrome.runtime.getContexts({ contextTypes: ['OFFSCREEN_DOCUMENT'] });

	if (open.length > 0) return;
	await chrome.offscreen.createDocument({
		url: OFFSCREEN,
		reasons: ['DOM_PARSER'],
		justification: 'reads download gate pages to walk them to their file'
	});
}

/** The walk, as the offscreen page answers it; a page that never answered sends the person to the gate. */
async function walk(url: string): Promise<WalkGateAnswer> {
	try {
		await openOffscreen();

		const answer: WalkGateAnswer | undefined = await chrome.runtime.sendMessage({
			type: 'WALK_GATE',
			url
		} satisfies WalkGateRequest);

		return answer ?? { ok: false, openUrl: url };
	} catch (_e) {
		return { ok: false, openUrl: url };
	}
}

/**
 * Walks one gate and saves its file, or opens the gate (or the file, when it was the transfer
 * that failed) beside the tab that asked, which is what the button did before it could walk.
 */
async function saveGate(url: string, from: chrome.tabs.Tab | undefined): Promise<SaveGateAnswer> {
	const answer = await walk(url);
	let openUrl = answer.ok ? answer.url : answer.openUrl;

	if (answer.ok) {
		try {
			await save(answer.url, { subfolder: answer.subfolder, filename: answer.filename });

			return { saved: true, mark: answer.mark };
		} catch (_e) {
			openUrl = answer.url;
		}
	}

	try {
		await chrome.tabs.create(from ? { url: openUrl, index: from.index + 1, openerTabId: from.id } : { url: openUrl });
	} catch (_e) {
		// The tab that asked is gone, and the person with it.
	}

	return { saved: false, mark: false };
}

function inTurn(url: string, from: chrome.tabs.Tab | undefined) {
	waiting += 1;
	const saved = turn.then(() => saveGate(url, from));

	turn = saved.then(closeIfIdle);

	return saved;
}

async function closeIfIdle() {
	waiting -= 1;

	if (waiting === 0) await chrome.offscreen.closeDocument().catch(() => {});
}

chrome.runtime.onMessage.addListener((message: SaveGateRequest | RoundTripRequest, sender, sendResponse) => {
	if (message?.type === 'SAVE_GATE') {
		void inTurn(message.url, sender.tab).then(sendResponse);

		return true;
	}

	// Only the offscreen page asks for a sign-in, and it has no tab.
	if (message?.type === 'ROUND_TRIP' && !sender.tab) {
		void roundTrip(message.url, { landing: message.landing }).then(sendResponse);

		return true;
	}

	return undefined;
});
