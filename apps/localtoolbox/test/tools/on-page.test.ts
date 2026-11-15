// on-page.ts: the background worker's run of gates done on their own page.
//
// The run and the outcomes live in chrome.storage.session, which is what these
// tests read back; the tab the run is in, and the messages the page gets, are
// the chrome mock's.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import { addPageApis, downloadOf, OWN_ID, tabOf, type PageApis } from '../helpers/page-apis.ts';
import { NEXT_AFTER_MS, OUTCOMES, Outcomes, onPage } from '../../src/tools/third-party-downloads/on-page.ts';
import type { GatePagePrefs, PageGate } from '$lib/types';

declare const chrome: ChromeMock;

const DROPLOUD = { url: 'https://droploud.com/gate/abc', permalink: '/a/front' };

const PYS = { url: 'https://pumpyoursound.com/f/pys/love-me-now/197974', permalink: '/a/love' };

const LAYLO = { url: 'https://laylo.com/hairitage/9PWcf', permalink: '' };

const HYPEDDIT = { url: 'https://hypeddit.com/886rp1', permalink: '/a/hype' };

/** The SoundCloud tab a page's button was pressed in: the third. */
const FROM = tabOf(7, 2);

/** The tab the run opens: what the chrome mock's tabs.create answers with. */
const RUN_TAB = tabOf(99);

let apis: PageApis;

let prefs: GatePagePrefs;

let pages: ReturnType<typeof onPage>;

beforeEach(() => {
	globalThis.chrome = createChrome();
	apis = addPageApis(chrome);
	prefs = { email: 'me@example.com', mark: true };
	pages = onPage(async () => prefs);
});

/** The run as the session holds it, or undefined for none. */
const run = async () => (await chrome.storage.session.get('gate-run'))['gate-run'];

/** What became of each gate, as the session holds it. */
const outcomes = async () => Outcomes.parse((await chrome.storage.session.get(OUTCOMES))[OUTCOMES]);

/** A run already going in RUN_TAB, as `open` leaves one. */
async function going(...gates: PageGate[]) {
	await pages.open(gates);
}

describe('open', () => {
	it('opens the first page gate beside the tab that asked, and keeps the rest', async () => {
		await pages.open([DROPLOUD, HYPEDDIT, PYS], FROM);

		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: DROPLOUD.url, index: 3, openerTabId: 7 });
		expect(await run()).toEqual({ tab: 99, gates: [DROPLOUD, PYS], of: 2 });
		expect(await outcomes()).toEqual({
			[DROPLOUD.url]: { state: 'opened', detail: "finish it on Droploud's page.", label: '' }
		});
	});

	it('opens a tab of its own when nothing asked from one', async () => {
		await pages.open([PYS]);

		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: PYS.url });
	});

	it('forgets what became of these gates before', async () => {
		await chrome.storage.session.set({
			[OUTCOMES]: {
				[DROPLOUD.url]: { state: 'done', detail: '', label: 'old.mp3' },
				[PYS.url]: { state: 'done', detail: '', label: 'old.mp3' },
				[LAYLO.url]: { state: 'sent', detail: 'kept', label: '' }
			}
		});

		await pages.open([DROPLOUD, PYS]);

		expect(await outcomes()).toEqual({
			[DROPLOUD.url]: { state: 'opened', detail: "finish it on Droploud's page.", label: '' },
			[LAYLO.url]: { state: 'sent', detail: 'kept', label: '' }
		});
	});

	it('does nothing for a run with no page gate in it', async () => {
		await pages.open([HYPEDDIT, { url: 'https://ffm.to/x', permalink: '' }]);
		await pages.open([]);

		expect(chrome.tabs.create).not.toHaveBeenCalled();
		expect(await run()).toBeUndefined();
	});
});

describe('next', () => {
	it('moves the tab on to the next gate', async () => {
		await going(DROPLOUD, PYS, LAYLO);

		await pages.next(RUN_TAB);

		expect(apis.update).toHaveBeenCalledWith(99, { url: PYS.url, active: true });
		expect(await run()).toEqual({ tab: 99, gates: [PYS, LAYLO], of: 3 });
		expect((await outcomes())[PYS.url]).toEqual({
			state: 'opened',
			detail: "finish it on Pump Your Sound's page.",
			label: ''
		});
	});

	it('ends the run after the last gate', async () => {
		await going(DROPLOUD);

		await pages.next();

		expect(await run()).toBeUndefined();
		expect(apis.update).not.toHaveBeenCalled();
	});

	it('is not moved on from another tab', async () => {
		await going(DROPLOUD, PYS);

		await pages.next(tabOf(5));

		expect(await run()).toEqual({ tab: 99, gates: [DROPLOUD, PYS], of: 2 });
	});

	it('does nothing with no run', async () => {
		await pages.next(RUN_TAB);

		expect(apis.update).not.toHaveBeenCalled();
	});

	it('ends the run when the tab has gone', async () => {
		await going(DROPLOUD, PYS);
		apis.update.mockRejectedValue(new Error('No tab with id: 99.'));

		await pages.next(RUN_TAB);

		expect(await run()).toBeUndefined();
	});
});

describe('hello', () => {
	it('tells the run’s tab what to help with, with the address for a page that has an email field', async () => {
		await going(DROPLOUD, PYS);

		expect(await pages.hello(RUN_TAB, 'https://droploud.com/track/abc')).toEqual({
			name: 'Droploud',
			say: expect.stringContaining('Droploud/'),
			steps: 'form.ds-gate-pane',
			email: 'me@example.com',
			emailField: 'form.ds-gate-pane input[type="email"]',
			signup: '',
			at: 1,
			of: 2
		});
	});

	it('hands no address to a page with no email field, and counts how far the run is', async () => {
		await going(DROPLOUD, PYS);
		await pages.next(RUN_TAB);

		expect(await pages.hello(RUN_TAB, PYS.url)).toMatchObject({ email: '', emailField: '', at: 2, of: 2 });
	});

	it('names the sign-up form on a page that ends in one', async () => {
		await going(LAYLO);

		expect(await pages.hello(RUN_TAB, LAYLO.url)).toMatchObject({ signup: 'form.laylo-rsvp-form', of: 1 });
	});

	it('answers a page on one of the site’s subdomains', async () => {
		await going(DROPLOUD);

		expect(await pages.hello(RUN_TAB, 'https://beta.droploud.com/gate/abc')).toMatchObject({ name: 'Droploud' });
	});

	it('answers null to any other tab, or the run’s tab on another site', async () => {
		expect(await pages.hello(RUN_TAB, DROPLOUD.url)).toBeNull();

		await going(DROPLOUD);
		expect(await pages.hello(tabOf(5), DROPLOUD.url)).toBeNull();
		expect(await pages.hello(undefined, DROPLOUD.url)).toBeNull();
		expect(await pages.hello(RUN_TAB, PYS.url)).toBeNull();
		expect(await pages.hello(RUN_TAB, undefined)).toBeNull();
	});

	it('answers null when the session cannot be read', async () => {
		await going(DROPLOUD);
		chrome.storage.session.get.mockRejectedValueOnce(new Error('storage gone'));

		expect(await pages.hello(RUN_TAB, DROPLOUD.url)).toBeNull();
	});
});

describe('signedUp, stop and closed', () => {
	it('says the person signed up on the gate open in the run’s tab', async () => {
		await going(LAYLO);

		await pages.signedUp(tabOf(5));
		expect((await outcomes())[LAYLO.url].state).toBe('opened');

		await pages.signedUp(RUN_TAB);
		expect((await outcomes())[LAYLO.url]).toEqual({
			state: 'sent',
			detail: "you RSVP'd on Laylo's page. the link comes by mail.",
			label: ''
		});
	});

	it('says nothing with no run', async () => {
		await pages.signedUp(RUN_TAB);

		expect(await outcomes()).toEqual({});
	});

	it('stops only for the run’s tab', async () => {
		await going(DROPLOUD);

		await pages.stop(tabOf(5));
		expect(await run()).toBeDefined();

		await pages.stop(RUN_TAB);
		expect(await run()).toBeUndefined();

		// Nothing left to stop.
		await pages.stop(RUN_TAB);
		expect(await run()).toBeUndefined();
	});

	it('ends the run when its tab closes, and not for another', async () => {
		await going(DROPLOUD);

		await pages.closed(5);
		expect(await run()).toBeDefined();

		await pages.closed(99);
		expect(await run()).toBeUndefined();
	});
});

describe('file', () => {
	const ITEM = {
		filename: '/Users/me/Downloads/Front to Back.mp3',
		referrer: 'https://droploud.com/',
		url: 'https://cdn.example.net/f.mp3',
		finalUrl: 'https://cdn.example.net/f.mp3'
	};

	const saved = () => chrome.tabs.sendMessage.mock.calls.map(([tab, message]) => [tab, message]);

	it('files the gate’s download in the site’s folder, says so on the page and marks the track', async () => {
		await going(DROPLOUD);

		expect(await pages.file(downloadOf(ITEM))).toEqual({
			filename: 'Droploud/Front to Back.mp3',
			conflictAction: 'uniquify'
		});
		await flush();

		expect((await outcomes())[DROPLOUD.url]).toEqual({ state: 'done', detail: '', label: 'Front to Back.mp3' });
		expect(saved()).toEqual([
			[99, { type: 'GATE_PAGE_SAVED', path: 'Droploud/Front to Back.mp3', more: false, mark: '/a/front' }]
		]);
		// The last gate: the run is over.
		expect(await run()).toBeUndefined();
	});

	it('leaves the mark out when saving is set not to mark', async () => {
		prefs = { email: '', mark: false };
		await going(DROPLOUD);

		await pages.file(downloadOf(ITEM));
		await flush();

		expect(saved()[0][1]).toMatchObject({ mark: '' });
	});

	it('knows the gate’s file by its address or where it ended up, subdomains too', async () => {
		await going(PYS);

		expect(
			await pages.file(
				downloadOf({
					filename: 'a.mp3',
					url: 'https://files.pumpyoursound.com/a.mp3'
				})
			)
		).toMatchObject({ filename: 'PumpYourSound/a.mp3' });

		await going(PYS);
		expect(
			await pages.file(
				downloadOf({
					filename: 'b.mp3',
					referrer: '',
					url: 'https://elsewhere.test/b',
					finalUrl: 'https://pumpyoursound.com/b.mp3'
				})
			)
		).toMatchObject({ filename: 'PumpYourSound/b.mp3' });
	});

	it('makes the browser’s name safe, and names a nameless file', async () => {
		await going(DROPLOUD);
		expect(await pages.file(downloadOf({ ...ITEM, filename: 'C:\\Downloads\\Remix: *FREE*?.wav' }))).toMatchObject({
			filename: 'Droploud/Remix- -FREE--.wav'
		});

		await going(DROPLOUD);
		expect(await pages.file(downloadOf({ ...ITEM, filename: '' }))).toMatchObject({
			filename: 'Droploud/download'
		});
	});

	it('opens the next gate a moment after a saved file', async () => {
		vi.useFakeTimers();
		await going(DROPLOUD, PYS);

		await pages.file(downloadOf(ITEM));
		await flush();
		expect(saved()[0][1]).toMatchObject({ more: true });
		expect(apis.update).not.toHaveBeenCalled();

		await vi.advanceTimersByTimeAsync(NEXT_AFTER_MS);
		await flush();
		expect(apis.update).toHaveBeenCalledWith(99, { url: PYS.url, active: true });
		expect(await run()).toEqual({ tab: 99, gates: [PYS], of: 2 });
	});

	it('still files it when the page has gone', async () => {
		await going(DROPLOUD);
		chrome.tabs.sendMessage.mockRejectedValue(new Error('No tab with id: 99.'));

		expect(await pages.file(downloadOf(ITEM))).toMatchObject({
			filename: 'Droploud/Front to Back.mp3'
		});
		await flush();
		expect((await outcomes())[DROPLOUD.url].state).toBe('done');
	});

	it('leaves every other download alone', async () => {
		expect(await pages.file(downloadOf(ITEM))).toBeUndefined();

		await going(DROPLOUD);
		expect(await pages.file(downloadOf({ ...ITEM, byExtensionId: OWN_ID }))).toBeUndefined();
		expect(
			await pages.file(
				downloadOf({
					filename: 'other.zip',
					referrer: 'https://example.org/',
					url: 'https://example.org/other.zip',
					finalUrl: 'https://example.org/other.zip'
				})
			)
		).toBeUndefined();
		expect(await pages.file(downloadOf({ filename: 'x.zip', url: 'https://notdroploud.com/x.zip' }))).toBeUndefined();
		await flush();
		expect(saved()).toEqual([]);
	});
});

describe('Outcomes', () => {
	it('reads anything that is not outcomes as none', () => {
		expect(Outcomes.parse(undefined)).toEqual({});
		expect(Outcomes.parse([1])).toEqual({});
		expect(Outcomes.parse({ [DROPLOUD.url]: { state: 'lost', detail: '', label: '' } })).toEqual({});
	});
});
