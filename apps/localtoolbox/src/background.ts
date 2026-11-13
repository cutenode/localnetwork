// The background worker, for what neither the page nor the popup can do by
// itself.
//
// Walking a Hypeddit gate when the Download button the content script put on
// the page is pressed on a link to one (content.ts, "Replacing SoundCloud's own
// download buttons"). The walk is the Downloader panel's own, site module and
// all, and neither of the two obvious places can run it. A content script
// fetches as the page it is in, so every gate is another origin it is refused;
// a service worker has no DOMParser, which the Hypeddit walk reads its page
// with. So the walk runs in an offscreen page (offscreen.ts), a document Chrome
// opens for exactly this, and this worker does the parts only it can: the tab a
// Spotify step is walked in, the download, and the tab the gate opens in when
// the walk ends anywhere but a file.
//
// Running a gate done on its own page (Droploud, Pump Your Sound, Laylo): the
// tab it is done in, the gates after it, and filing the download the page
// starts (tools/third-party-downloads/on-page.ts). The address gate-page.ts
// fills in, and whether a saved file marks its track, are settings in the
// popup's localStorage, so the offscreen page reads them for it.

import { roundTrip } from '$lib/chrome.ts';
import type {
	GatePagePrefs,
	GatePageRequest,
	OffscreenRequest,
	OpenGatesRequest,
	RoundTripRequest,
	SaveGateAnswer,
	SaveGateRequest,
	WalkGateAnswer
} from '$lib/types';
import { dropOldProfiles } from './tools/artists/old-profiles.ts';
import { onPage } from './tools/third-party-downloads/on-page.ts';
import { save } from './tools/third-party-downloads/save.ts';
import { siteFor } from './tools/third-party-downloads/sites/index.ts';

const OFFSCREEN = 'offscreen.html';

// One errand at a time, for the panel's reason: several of these sites hand out
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
		reasons: ['DOM_PARSER', 'LOCAL_STORAGE'],
		justification: 'reads download gate pages to walk them to their file, and the settings they use'
	});
}

/** One errand for the offscreen page, in turn; undefined when it never answered. */
function ask<T>(message: OffscreenRequest): Promise<T | undefined> {
	waiting += 1;

	const answered = turn.then(async (): Promise<T | undefined> => {
		try {
			await openOffscreen();

			// What the offscreen page answers each request with (offscreen.ts).
			return await chrome.runtime.sendMessage(message);
		} catch (_e) {
			return undefined;
		}
	});

	turn = answered.then(closeIfIdle);

	return answered;
}

async function closeIfIdle() {
	waiting -= 1;

	if (waiting === 0) await chrome.offscreen.closeDocument().catch(() => {});
}

const pages = onPage(async () => (await ask<GatePagePrefs>({ type: 'READ_PREFS' })) ?? { email: '', mark: false });

/**
 * Walks one gate and saves its file, or opens the gate (or the file, when it was the transfer
 * that failed) beside the tab that asked. A gate done on its own page is opened there, with the
 * page's help.
 */
async function saveGate(request: SaveGateRequest, from: chrome.tabs.Tab | undefined): Promise<SaveGateAnswer> {
	if (siteFor(request.url)?.how === 'page') {
		await pages.open([{ url: request.url, permalink: request.permalink }], from);

		return { saved: false, mark: false };
	}

	const answer = (await ask<WalkGateAnswer>({ type: 'WALK_GATE', url: request.url })) ?? {
		ok: false,
		openUrl: request.url
	};

	let openUrl = answer.ok ? answer.url : answer.openUrl;

	if (answer.ok) {
		try {
			await save(answer.url, { subfolder: answer.subfolder });

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

chrome.runtime.onMessage.addListener(
	(message: SaveGateRequest | RoundTripRequest | OpenGatesRequest | GatePageRequest, sender, sendResponse) => {
		switch (message?.type) {
			case 'SAVE_GATE':
				void saveGate(message, sender.tab).then(sendResponse);

				return true;

			// Only the offscreen page asks for a sign-in, and it has no tab.
			case 'ROUND_TRIP':
				if (sender.tab) return undefined;
				void roundTrip(message.url, { landing: message.landing }).then(sendResponse);

				return true;

			case 'OPEN_GATES':
				void pages.open(message.gates).then(() => sendResponse(true));

				return true;

			case 'GATE_PAGE_HELLO':
				void pages.hello(sender.tab, sender.url).then(sendResponse);

				return true;

			case 'GATE_PAGE_NEXT':
				void pages.next(sender.tab);

				return undefined;

			case 'GATE_PAGE_STOP':
				void pages.stop(sender.tab);

				return undefined;

			case 'GATE_PAGE_SIGNED_UP':
				void pages.signedUp(sender.tab);

				return undefined;

			default:
				return undefined;
		}
	}
);

chrome.tabs.onRemoved.addListener((tabId) => void pages.closed(tabId));

// Every download passes through here, and only the open gate's file is given a
// name: the rest are answered with nothing, which leaves the browser's own.
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
	void pages.file(item).then((where) => suggest(where));

	return true;
});

// Versions before this one kept every track's download details for a month,
// under `details` in the local area, and every listed artist's profile on the
// artists list for good. Details last as long as the tab that loaded them now
// (content.ts) and profiles as long as the browser session
// (artists/artists.svelte.ts), so what an older version left on the disk is
// cleared once, when this one is installed over it.
chrome.runtime.onInstalled.addListener(() => {
	void chrome.storage.local.remove('details');
	void dropOldProfiles();
});
