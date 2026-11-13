// background.ts: the worker that walks a gate for the page's Download button,
// and runs the gates done on their own page.
//
// The walk itself is the offscreen page's (test/offscreen.test.ts); here it is
// whatever that page answers WALK_GATE with, and what is under test is what the
// worker does around it: the offscreen page opened and closed, the file saved,
// the gate opened when there is no file, and one walk at a time. The run of
// page gates is on-page.ts's (test/tools/on-page.test.ts); here it is how the
// worker's messages and events reach it. Last, what it clears when it is
// installed over an older version.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock, type PageReply, type Seed } from './helpers/chrome.ts';
import { flush } from './helpers/flush.ts';
import { addPageApis, type PageApis } from './helpers/page-apis.ts';
import type { GatePagePrefs, WalkGateAnswer } from '$lib/types';

declare const chrome: ChromeMock;

const GATE = 'https://hypeddit.com/886rp1';

const FILE = 'https://gates.s3.amazonaws.com/886rp1/One.wav';

/** The SoundCloud tab the button was pressed in: the third, so a tab opened beside it is the fourth. */
const TAB: chrome.tabs.Tab = {
	id: 7,
	index: 2,
	windowId: 1,
	groupId: -1,
	pinned: false,
	highlighted: true,
	active: true,
	frozen: false,
	incognito: false,
	selected: true,
	discarded: false,
	autoDiscardable: true,
	lastAccessed: 0
};

const walked = (over: Partial<Extract<WalkGateAnswer, { ok: true }>> = {}): WalkGateAnswer => ({
	ok: true,
	url: FILE,
	subfolder: 'Hypeddit',
	mark: true,
	...over
});

let apis: PageApis;

/**
 * Starts the worker, with the offscreen page answering every walk with `answer` and every read
 * of the settings with `prefs`.
 */
async function start(
	answer: () => Promise<PageReply> = async () => walked(),
	// null: the offscreen page never answers.
	prefs: GatePagePrefs | null = { email: 'me@example.com', mark: true }
) {
	globalThis.chrome = createChrome();
	apis = addPageApis(chrome);
	chrome.runtime.sendMessage.mockImplementation(async (message) => {
		if (message.type === 'WALK_GATE') return answer();

		return message.type === 'READ_PREFS' && prefs ? { ...prefs } : undefined;
	});
	vi.resetModules();
	await import('../src/background.ts');
}

const press = (url = GATE, sender: chrome.runtime.MessageSender = { id: 'test', tab: TAB }) =>
	chrome.testSendMessage({ type: 'SAVE_GATE', url, permalink: '/a/one' }, sender);

const walks = () => chrome.runtime.sendMessage.mock.calls.filter(([message]) => message.type === 'WALK_GATE');

describe('a gate the offscreen page walks to a file', () => {
	it('saves it in the site’s folder, under its own name, and says whether to mark it', async () => {
		await start();

		expect(await press()).toEqual({ saved: true, mark: true });
		expect(walks()).toEqual([[{ type: 'WALK_GATE', url: GATE }]]);
		expect(chrome.downloads.download).toHaveBeenCalledWith({ url: FILE, filename: 'Hypeddit/One.wav' });
		expect(chrome.tabs.create).not.toHaveBeenCalled();
	});

	it('passes on a mark setting that is off', async () => {
		await start(async () => walked({ mark: false }));

		expect(await press()).toEqual({ saved: true, mark: false });
	});

	it('opens the offscreen page to read it with, and closes it once nothing is waiting', async () => {
		await start();

		await press();
		await flush();
		expect(chrome.offscreen.createDocument).toHaveBeenCalledWith({
			url: 'offscreen.html',
			reasons: ['DOM_PARSER', 'LOCAL_STORAGE'],
			justification: expect.any(String)
		});
		expect(chrome.offscreen.closeDocument).toHaveBeenCalledTimes(1);
	});

	it('uses an offscreen page that is already open', async () => {
		await start();
		chrome.runtime.getContexts.mockResolvedValue([{ contextType: 'OFFSCREEN_DOCUMENT' }]);

		expect(await press()).toEqual({ saved: true, mark: true });
		expect(chrome.offscreen.createDocument).not.toHaveBeenCalled();
	});

	it('does not mind an offscreen page that is already gone when it closes it', async () => {
		await start();
		chrome.offscreen.closeDocument.mockRejectedValue(new Error('No current offscreen document.'));

		expect(await press()).toEqual({ saved: true, mark: true });
		await flush();
		// Still answering: the next press is walked as the first was.
		expect(await press()).toEqual({ saved: true, mark: true });
	});

	it('opens the file in a tab when Chrome will not take the download', async () => {
		await start();
		chrome.downloads.download.mockRejectedValue(new Error('Download failed'));

		expect(await press()).toEqual({ saved: false, mark: false });
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: FILE, index: 3, openerTabId: 7 });
	});
});

describe('a gate that does not end in a file here', () => {
	it('opens where the walk says to, beside the tab that asked', async () => {
		await start(async () => ({ ok: false, openUrl: 'https://dropbox.test/folder' }));

		expect(await press()).toEqual({ saved: false, mark: false });
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: 'https://dropbox.test/folder', index: 3, openerTabId: 7 });
		expect(chrome.downloads.download).not.toHaveBeenCalled();
	});

	it('opens the gate when the offscreen page never answers', async () => {
		await start(async () => undefined);

		expect(await press()).toEqual({ saved: false, mark: false });
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: GATE, index: 3, openerTabId: 7 });
	});

	it('opens the gate when there is no offscreen page to ask', async () => {
		await start();
		chrome.offscreen.createDocument.mockRejectedValue(new Error('Only a single offscreen document may be created.'));

		expect(await press()).toEqual({ saved: false, mark: false });
		expect(walks()).toEqual([]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: GATE, index: 3, openerTabId: 7 });
	});

	it('opens a tab of its own for a page that asked from no tab', async () => {
		await start(async () => ({ ok: false, openUrl: GATE }));

		await press(GATE, { id: 'test' });
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: GATE });
	});

	it('still answers when the tab cannot be opened', async () => {
		await start(async () => ({ ok: false, openUrl: GATE }));
		chrome.tabs.create.mockRejectedValue(new Error('No tab with id: 7.'));

		expect(await press()).toEqual({ saved: false, mark: false });
	});
});

describe('several presses', () => {
	it('walks one gate at a time, and closes the page after the last', async () => {
		let finish: (answer: WalkGateAnswer) => void = () => {};

		await start(() => new Promise((resolve) => (finish = resolve)));

		const first = press(GATE);
		const second = press('https://hypeddit.com/two');
		await flush();
		expect(walks()).toHaveLength(1);

		finish(walked());
		expect(await first).toEqual({ saved: true, mark: true });
		await flush();
		expect(walks()).toHaveLength(2);
		expect(chrome.offscreen.closeDocument).not.toHaveBeenCalled();

		finish(walked({ url: `${FILE}?two` }));
		expect(await second).toEqual({ saved: true, mark: true });
		await flush();
		expect(chrome.offscreen.closeDocument).toHaveBeenCalledTimes(1);
	});
});

describe('the sign-in the offscreen page cannot walk', () => {
	const AUTHORIZE = 'https://accounts.spotify.com/authorize?state=THIS-GATE';

	it('walks it in a tab and says whether the tab came back', async () => {
		vi.useFakeTimers();
		await start();
		chrome.tabs.get.mockResolvedValue({ id: 99, url: 'https://hypeddit.com/886rp1' });

		const trip = chrome.testSendMessage(
			{ type: 'ROUND_TRIP', url: AUTHORIZE, landing: 'https://hypeddit.com/' },
			{ id: 'test' }
		);

		await vi.advanceTimersByTimeAsync(300);
		expect(await trip).toBe(true);
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: AUTHORIZE, active: false });
	});

	it('is not walked for a page in a tab', async () => {
		await start();

		expect(
			await chrome.testSendMessage(
				{ type: 'ROUND_TRIP', url: AUTHORIZE, landing: 'https://hypeddit.com/' },
				{ id: 'test', tab: TAB }
			)
		).toBeUndefined();
		expect(chrome.tabs.create).not.toHaveBeenCalled();
	});
});

it('leaves every other message to whoever it is for', async () => {
	await start();

	expect(await chrome.testSendMessage({ type: 'EXTRACT_TRACKS' })).toBeUndefined();
	expect(await chrome.testSendMessage(null)).toBeUndefined();
});

describe('gates done on their own page', () => {
	const DROPLOUD = 'https://droploud.com/gate/abc';

	const PYS = 'https://pumpyoursound.com/f/pys/love-me-now/197974';

	/** The tab the run opens: what the chrome mock's tabs.create answers with. */
	const RUN = { id: 'test', tab: { ...TAB, id: 99 }, url: DROPLOUD };

	const run = async () => (await chrome.storage.session.get('gate-run'))['gate-run'];

	it('opens one from the page’s button beside its tab, without walking it', async () => {
		await start();

		expect(await press(DROPLOUD)).toEqual({ saved: false, mark: false });
		expect(walks()).toEqual([]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: DROPLOUD, index: 3, openerTabId: 7 });
		expect(await run()).toEqual({ tab: 99, gates: [{ url: DROPLOUD, permalink: '/a/one' }], of: 1 });
	});

	it('opens the Downloader’s run of them, and answers once it has', async () => {
		await start();

		const gates = [
			{ url: DROPLOUD, permalink: '/a/one' },
			{ url: PYS, permalink: '/a/two' }
		];

		expect(await chrome.testSendMessage({ type: 'OPEN_GATES', gates })).toBe(true);
		expect(await run()).toEqual({ tab: 99, gates, of: 2 });
	});

	it('tells the run’s page what to help with, with the address the offscreen page reads', async () => {
		await start();
		await press(DROPLOUD);

		expect(await chrome.testSendMessage({ type: 'GATE_PAGE_HELLO' }, RUN)).toMatchObject({
			name: 'Droploud',
			email: 'me@example.com'
		});
	});

	it('helps with no address when the offscreen page never answers', async () => {
		await start(undefined, null);
		await press(DROPLOUD);

		expect(await chrome.testSendMessage({ type: 'GATE_PAGE_HELLO' }, RUN)).toMatchObject({ email: '' });
	});

	it('moves on, stops and hears a sign-up when the run’s page says so', async () => {
		await start();
		await chrome.testSendMessage({
			type: 'OPEN_GATES',
			gates: [
				{ url: DROPLOUD, permalink: '' },
				{ url: 'https://laylo.com/hairitage/9PWcf', permalink: '' }
			]
		});

		expect(await chrome.testSendMessage({ type: 'GATE_PAGE_NEXT' }, RUN)).toBeUndefined();
		await flush();
		expect(apis.update).toHaveBeenCalledWith(99, { url: 'https://laylo.com/hairitage/9PWcf', active: true });

		expect(await chrome.testSendMessage({ type: 'GATE_PAGE_SIGNED_UP' }, RUN)).toBeUndefined();
		await flush();
		expect((await chrome.storage.session.get('gate-outcomes'))['gate-outcomes']).toMatchObject({
			'https://laylo.com/hairitage/9PWcf': { state: 'sent' }
		});

		expect(await chrome.testSendMessage({ type: 'GATE_PAGE_STOP' }, RUN)).toBeUndefined();
		await flush();
		expect(await run()).toBeUndefined();
	});

	it('ends the run when its tab closes', async () => {
		await start();
		await press(DROPLOUD);

		apis.close(99);
		await flush();
		expect(await run()).toBeUndefined();
	});

	it('names the run’s download for its folder, and no other', async () => {
		await start();
		await press(DROPLOUD);

		expect(await apis.name({ filename: 'One.mp3', referrer: 'https://droploud.com/' })).toEqual({
			filename: 'Droploud/One.mp3',
			conflictAction: 'uniquify'
		});
		expect(await apis.name({ filename: 'other.zip', referrer: 'https://example.org/' })).toBeUndefined();
	});
});

// Versions before this one kept every track's download details for a month,
// under `details` in the local area. They last as long as the tab that loaded
// them now (content.ts), so installing this version over an older one clears
// that key once, and nothing else the extension keeps beside it.
describe('installed over an older version', () => {
	const install = async (storage: Seed) => {
		globalThis.chrome = createChrome({ storage });
		addPageApis(chrome);
		vi.resetModules();
		await import('../src/background.ts');
		const [[installed]] = chrome.runtime.onInstalled.addListener.mock.calls;
		installed();
		await flush();
	};

	it('clears the details cache it left, and keeps the marks and the lists', async () => {
		const kept = {
			downloaded: { '/a/b': '2026-09-08T12:00:00.000Z' },
			listed: [],
			artists: []
		};

		await install({ ...kept, details: { '/a/b': { unavailable: true, at: 1 } } });

		expect(chrome.testStore()).toEqual(kept);
	});

	it('changes nothing where there was no cache to clear', async () => {
		await install({ downloaded: { '/a/b': '2026-09-08T12:00:00.000Z' } });

		expect(chrome.testStore()).toEqual({ downloaded: { '/a/b': '2026-09-08T12:00:00.000Z' } });
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});
});
