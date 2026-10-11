// The browser APIs the panels use, behind small functions that never throw.
//
// A popup runs in a window that can be opened over any tab, including one the
// content script was never injected into, so every one of these calls has a
// perfectly ordinary way of failing. Answering with a value the caller can
// branch on keeps that handling in the panel, where the wording of what went
// wrong belongs.

import type { PageAnswers, PageRequest } from '$lib/types';

export const SOUNDCLOUD_URL = /^https:\/\/soundcloud\.com\//;

/** @param url a tab's, which is missing where the host is not permitted */
export const isSoundCloudUrl = (url: string | undefined) => SOUNDCLOUD_URL.test(url || '');

const queryActiveTab = async () => {
	try {
		const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });

		return tab ?? null;
	} catch (_e) {
		return null;
	}
};

// A read started before anything asked for one. The popup spends its first
// few milliseconds compiling a panel's chunk and reading the session, and the
// tab lookup is a round trip to the browser process that can happen during
// them rather than after (see warmActiveTab, and $lib/soundcloud.js).
let warmed: Promise<chrome.tabs.Tab | null> | null = null;

/**
 * Starts reading the active tab now, for whoever asks for it next.
 *
 * One read, handed to the first `activeTab()` that comes along; a second call
 * reads again. Nothing here has to be undone if the answer is never taken —
 * the popup that started it is gone by then.
 */
export function warmActiveTab() {
	warmed ??= queryActiveTab();
}

/** The tab the popup was opened over, or null when it cannot be read. */
export function activeTab() {
	warmed ??= queryActiveTab();
	const held = warmed;
	// Callers in the same turn share one lookup: the Manager tool asks the
	// page for two things at boot (its tracks, and their download details) and
	// both want the same tab, so that is one round trip to the browser rather
	// than two on the path being measured. It is let go of once it has
	// answered, so a later read — the Downloader collecting again after a run —
	// asks afresh rather than trusting a tab that may have changed since.
	// queryActiveTab never rejects, so nothing is lost by not waiting here.
	void held.finally(() => {
		if (warmed === held) warmed = null;
	});

	return held;
}

/**
 * Asks the content script something.
 *
 * `reached` is false when the message could not be delivered at all — the
 * usual cause being a tab that was open before the extension was installed or
 * updated, which has no content script in it until it is reloaded.
 *
 * `extra` rides along with the type, for a question that names what it is
 * about (the artists a lookup is for); most questions are the type alone.
 *
 * `response` is null whenever no answer came back, delivered or not.
 */
export async function askPage<T extends keyof PageAnswers>(
	tabId: number,
	type: T,
	extra: Partial<Omit<Extract<PageRequest, { type: T }>, 'type'>> = {}
): Promise<{ reached: boolean; response: PageAnswers[T] | null }> {
	try {
		const response = await chrome.tabs.sendMessage(tabId, { type, ...extra });

		return { reached: true, response: response ?? null };
	} catch (_e) {
		return { reached: false, response: null };
	}
}

/** @param options `background` opens it behind the popup */
export function openTab(url: string, { background = false }: { background?: boolean } = {}) {
	return chrome.tabs.create({ url, active: !background });
}

/**
 * Sends a tab out to `url` and waits for it to come back to `landing`.
 *
 * This is how a sign-in that a fetch cannot do gets done anyway: an OAuth
 * round trip is a chain of redirects through a site the extension has no
 * business holding a session for, and the only thing that can walk it is a
 * tab. So one is opened behind the popup, watched until it arrives back where
 * the round trip was supposed to leave it, and closed again.
 *
 * It is quiet by design. A visitor who has already said yes to the app once is
 * sent straight back, which takes about a second and needs no clicking; one
 * who has not gets a login or a consent screen the background tab cannot ask
 * them to answer, and that is the `false` case — the caller's job is then to
 * say so, not to steal the window.
 *
 * A tab's URL is only legible to an extension that has permission for the
 * host, which is why `landing` is somewhere in `host_permissions`: the middle
 * of the trip is invisible here, and only the end of it has to be seen.
 *
 * @param url where to send the tab
 * @param options `landing` is matched as a prefix of the tab's URL. Every caller names one:
 *   without it the trip waits out the whole timeout and fails.
 * @returns whether it arrived before the timeout
 */
export async function roundTrip(
	url: string,
	// @ts-expect-error -- the `{}` default falls short of the contract above; no caller relies on it.
	{
		landing,
		timeout = 12000,
		every = 250
	}: {
		landing: string;
		timeout?: number;
		every?: number;
	} = {}
): Promise<boolean> {
	let tab: chrome.tabs.Tab;

	try {
		tab = await chrome.tabs.create({ url, active: false });
	} catch (_e) {
		return false;
	}

	let arrived = false;

	// Counted rather than clocked: the wait is a fixed number of looks, which is
	// the same thing in a browser and a knowable thing in a test.
	for (let look = Math.ceil(timeout / every); look > 0; look -= 1) {
		await new Promise((resume) => setTimeout(resume, every));
		let now: chrome.tabs.Tab;

		try {
			now = await chrome.tabs.get(tab.id!);
		} catch (_e) {
			// Closed under us — by the person watching it, or by the browser.
			return false;
		}

		if ((now.url || now.pendingUrl || '').startsWith(landing)) {
			arrived = true;
			break;
		}
	}

	try {
		await chrome.tabs.remove(tab.id!);
	} catch (_e) {
		// Already gone.
	}

	return arrived;
}
