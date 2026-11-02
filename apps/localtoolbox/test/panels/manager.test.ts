// src/tools/manager/Panel.svelte — the Manager panel, against
// the real downloaded-store.ts and state-transfer.ts.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
	addPage,
	button,
	buttons,
	byLabel,
	chooseOption,
	click,
	extraction,
	listedTracks,
	mountPanel,
	openSelect,
	panelChrome,
	reopenPanel,
	rows,
	settle,
	text,
	track,
	type,
	type Target
} from '../helpers/panel.ts';
import type { AnswerFields, ChromeMock, PageReply } from '../helpers/chrome.ts';
import type { DetailsProgress, KnownDetails, TrackDetails } from '$lib/types';

declare const chrome: ChromeMock;

// The tool keeps a list of its own and nothing goes onto it by itself, so
// opening the panel and putting the tab's tracks on the list are two steps.
// `open` is the first alone — what somebody sees on any tab — and `load` is
// both, which is where most of what there is to test starts.
const open = () => mountPanel('manager');

const load = async () => {
	const mounted = await open();
	// Only when the tab has something to offer: plenty below opens the panel
	// over a tab with no playlist on it, which is a thing the tool now does.
	const add = addButton();

	if (add && !add.disabled) await addPage();

	return mounted;
};

/** The add bar's button for the active tab. */
const addButton = () => buttons().find((b) => (b.getAttribute('aria-label') ?? '').endsWith('from this tab'));

/** The add bar's button for the session queue. */
const addQueueButton = () =>
	buttons().find((b) => (b.getAttribute('aria-label') ?? '').startsWith('add the session queue'));

const ISO = '2026-09-08T12:00:00.000Z';

/** What every track's details start from. */
const DETAILS: KnownDetails = {
	id: 1,
	permalink: '/x/y',
	downloadable: false,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	at: 1
};

const details = (extra: Partial<TrackDetails> = {}) => ({ ...DETAILS, ...extra });

// The panel's answer: extraction plus per-track details.
const answer = (tracks: object[], extra: AnswerFields = {}) =>
	extraction({ tracks, lookupFailed: 0, resumed: false, error: '', ...extra });

const meta = () => document.querySelector('header p')?.textContent.trim() ?? '';

const clean = (node: Target) => node!.textContent.replace(/\s+/g, ' ').trim();

// What a row carries besides the buttons every row has anyway: the cover art
// it copies, the two toggles (queue and mark), and the chevron that opens it
// (named after the track, so it is matched by its shape rather than listed —
// see "opening a row" for what it opens into).
const EVERY_ROW = new Set(['copy the cover art', 'add to the session queue', 'mark downloaded']);

const isEveryRow = (b: Element) => {
	const label = b.getAttribute('aria-label') ?? '';

	return EVERY_ROW.has(label) || label.startsWith('about ');
};

// Every button on a row is now an icon, so what stands for one here is its
// accessible name — the words on it where it has any (the search badges).
const rowButtons = (li: Element) =>
	[...li.querySelectorAll('button')].flatMap((b) => (isEveryRow(b) ? [] : [clean(b) || b.getAttribute('aria-label')]));

const sections = () =>
	[...document.querySelectorAll('section')].map((section) => {
		const heading = section.querySelector('h2');

		return {
			heading: heading ? heading.firstChild!.textContent!.trim() : null,
			count: heading ? clean(heading.querySelector('span')) : null,
			hint: section.querySelector(':scope > p')?.textContent.replace(/\s+/g, ' ').trim() ?? null,
			rows: [...section.querySelectorAll('li')].map((li) => ({
				permalink: li.dataset.permalink,
				detail: li.querySelectorAll('p')[2]?.textContent.replace(/\s+/g, ' ').trim() ?? null,
				buttons: rowButtons(li)
			}))
		};
	});

const rowFor = (permalink: string, nth = 0) => document.querySelectorAll(`li[data-permalink="${permalink}"]`)[nth];

const isMarked = (permalink: string, nth = 0) => markButton(permalink, nth)!.getAttribute('aria-pressed') === 'true';

const markButton = (permalink: string, nth = 0) =>
	rowFor(permalink, nth).querySelector('button[aria-label="mark downloaded"]');

const queueButton = (permalink: string, nth = 0) =>
	rowFor(permalink, nth).querySelector('button[aria-label="add to the session queue"]');

const isQueued = (permalink: string, nth = 0) => queueButton(permalink, nth)!.getAttribute('aria-pressed') === 'true';

// The queued tracks, as they are written to session storage.
const queued = () => (chrome.testStore('session').queue ?? []).map((item) => item.permalink);

// Every button on a row except the ones every row has: the download buttons,
// or the search badges of a track with no links.
const openButtons = (permalink: string, nth = 0) =>
	[...rowFor(permalink, nth).querySelectorAll('button')].filter((b) => !isEveryRow(b));

const clearButton = () =>
	[...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => clean(b).startsWith('unmark'));

// --- selecting a batch -------------------------------------------------------

const selectButton = () => byLabel('select several tracks');

const doneButton = () => batchButton('done');

const tickBox = (permalink: string, nth = 0) => rowFor(permalink, nth).querySelector('[role="checkbox"]');

const isTicked = (permalink: string, nth = 0) => tickBox(permalink, nth)!.getAttribute('aria-checked') === 'true';

/** The batch bar's buttons, by the words on them. */
const batchButton = (label: string) =>
	[...document.querySelectorAll('button')].find((b) => clean(b) === label && !b.closest('header') && !b.closest('li'));

const selectAllBox = () => document.querySelector('[aria-label="select every track shown"]');

/** The batch bar's button for the tracks the tab in front is showing. */
const pageTickButton = () => [...document.querySelectorAll('button')].find((b) => /tick this page/.test(clean(b)));

// Writes to session storage that were the queue's own: the selection is kept
// there as well, so counting every write would count the ticking too.
const queueWrites = () => chrome.storage.session.set.mock.calls.filter(([written]) => 'queue' in written).length;

/** Turns the tick boxes on and ticks the rows named. */
const tick = async (...permalinks: string[]) => {
	for (const permalink of permalinks) await click(tickBox(permalink));
};

describe('opening the panel', () => {
	// The whole point of the change: the tool is a list of its own, so it opens
	// on any tab at all — including the gate tab a row has just sent you to.
	it('opens on any page, with the list and no page read at all', async () => {
		for (const tab of [null, { id: 1, url: 'https://example.com/' }, { id: 2 }]) {
			panelChrome({ tab });
			await open();
			expect(text()).toMatch(/nothing on the list yet/);
			expect(text()).toMatch(/not a SoundCloud playlist, likes or track page/);
			expect(addButton()!.disabled).toBe(true);
			expect(chrome.tabs.sendMessage).not.toHaveBeenCalled();
			document.body.innerHTML = '';
		}

		panelChrome();
		chrome.tabs.query.mockRejectedValue(new Error('no window'));
		await open();
		expect(text()).toMatch(/not a SoundCloud playlist, likes or track page/);
	});

	// Nothing is asked of SoundCloud until somebody presses add. Reading the
	// page's own rows is free — it is a DOM read in the tab — and is what the
	// button counts; the lookup is the part that costs.
	it('reads the tab to describe the button, and looks nothing up', async () => {
		panelChrome({ response: answer([track('A', 'One'), track('B', 'Two')]) });
		await open();
		expect(chrome.tabs.sendMessage).toHaveBeenCalledWith(7, { type: 'EXTRACT_TRACKS' });
		expect(chrome.tabs.sendMessage).not.toHaveBeenCalledWith(7, {
			type: 'GET_DOWNLOAD_DETAILS'
		});
		expect(listedTracks()).toEqual([]);
		expect(rows()).toHaveLength(0);
		expect(text()).toMatch(/My Playlist/);
		expect(text()).toMatch(/by Curator · 2 tracks/);
		expect(clean(addButton())).toBe('add 2');
	});

	it('says what it holds, and offers the queue as a second source, on a tab with nothing on it', async () => {
		panelChrome({
			tab: { id: 1, url: 'https://hypeddit.com/gate' },
			session: { queue: [{ permalink: '/a/one', title: 'One', artist: 'A' }] }
		});
		await open();
		expect(addButton()!.disabled).toBe(true);
		expect(clean(addQueueButton())).toBe('queue (1)');

		await click(addQueueButton());
		expect(listedTracks()).toEqual(['/a/one']);
		expect(rows()).toHaveLength(1);
		expect(text()).toMatch(/added 1 track from the session queue/);
	});

	// Nothing is asked of SoundCloud for the queue: the lookup runs on the page a
	// track is on, and the point of this source is that that page need not be
	// open. On a SoundCloud tab, where a lookup *could* run, it still does not.
	it('adds the queue without looking anything up, even on a SoundCloud tab', async () => {
		panelChrome({
			response: answer([track('B', 'Two')]),
			session: { queue: [{ permalink: '/a/one', title: 'One', artist: 'A' }] }
		});
		await open();
		await click(addQueueButton());
		expect(listedTracks()).toEqual(['/a/one']);
		expect(chrome.tabs.sendMessage.mock.calls.map(([, message]) => message.type)).toEqual(['EXTRACT_TRACKS']);
	});

	it('keeps the list across openings, and adds a second page onto the end of it', async () => {
		panelChrome({ response: answer([track('A', 'One', { details: details({ downloadable: true }) })]) });
		await load();
		expect(listedTracks()).toEqual(['/a/one']);

		chrome.tabs.sendMessage.mockResolvedValue(
			answer([track('B', 'Two', { details: details({ downloadable: true }) })], {
				title: 'Another Playlist'
			})
		);
		await reopenPanel('manager');
		// Still there, without anything being read again.
		expect(rows()).toHaveLength(1);
		expect(clean(addButton())).toBe('add 1');

		await addPage();
		expect(listedTracks()).toEqual(['/a/one', '/b/two']);
		expect(text()).toMatch(/added 1 track from Another Playlist/);
	});

	// A page whose tracks are all on the list already: the button says what it
	// would really do, which is bring their answers up to date.
	it('offers to refresh a page it already holds', async () => {
		panelChrome({ response: answer([track('A', 'One', { details: details({ downloadable: true }) })]) });
		await load();
		await reopenPanel('manager');
		expect(clean(addButton())).toBe('refresh 1');
		await addPage();
		expect(listedTracks()).toEqual(['/a/one']);
		expect(text()).toMatch(/refreshed 1 track from My Playlist/);
	});
});

describe('adding a page', () => {
	it('shows progress while the content script looks the tracks up, until the answer is in', async () => {
		let answerNow!: (value: PageReply) => void;
		panelChrome({ response: answer([track('A', 'B')]) });
		await open();
		chrome.tabs.sendMessage.mockReturnValue(new Promise((resolve) => (answerNow = resolve)));
		await click(addButton());
		// The button says what it is doing, and the row is already up saying it
		// has not been checked; the bar under the header appears once the content
		// script reports how far along it is.
		expect(clean(addButton())).toBe('adding…');
		expect(rows()).toHaveLength(1);
		expect(chrome.tabs.sendMessage).toHaveBeenCalledWith(7, { type: 'GET_DOWNLOAD_DETAILS' });

		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 3,
			total: 10,
			resumed: false
		});
		await settle();
		expect(text()).toMatch(/checking download details…/);
		expect(text()).toMatch(/3 \/ 10/);

		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 4,
			total: 10,
			resumed: true
		});
		await settle();
		expect(text()).toMatch(/4 \/ 10/);
		expect(text()).toMatch(/\(already running\)/);

		await chrome.testSendMessage({ type: 'SOMETHING_ELSE' });
		await chrome.testSendMessage(null);
		await settle();
		expect(text()).toMatch(/4 \/ 10/);

		answerNow(answer([track('A', 'B', { details: details({ downloadable: true }) })]));
		await settle();
		expect(text()).not.toMatch(/checking download details/);

		// A report arriving after the add is over — another popup's lookup, still
		// running — is not this add's: it neither brings the bar back nor paints
		// its answers over the ones the list now holds.
		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 9,
			total: 10,
			details: [details({ permalink: '/a/b' })]
		});
		await settle();
		expect(text()).not.toMatch(/checking download details/);
		expect(meta()).toBe('1 track · 1 download link');
	});

	// Nothing to add from, so nothing is added and the reason is on screen
	// before the button is even pressed.
	it.each([
		['a tab that will not answer', new Error('nope'), /can’t reach the page — reload the tab/],
		['no answer at all', undefined, /couldn’t read the page/],
		['a failed read', { ok: false, error: 'boom' }, /couldn’t read the page/],
		['an empty playlist', answer([], { pageType: 'playlist' }), /no tracks here/],
		[
			'a likes page still loading',
			answer([], { pageType: 'likes' }),
			/no tracks here\. let the likes page finish loading/
		],
		[
			'a likes page of only playlists',
			answer([], { pageType: 'likes', skippedPlaylists: 2 }),
			/only liked playlists here \(2 skipped\)/
		],
		['some other page', answer([], { pageType: 'other' }), /that’s not a playlist, likes or track page/]
	])('says why there is nothing to add: %s', async (_why, response, message) => {
		panelChrome({ response });
		await open();
		expect(text()).toMatch(message);
		expect(addButton()!.disabled).toBe(true);
		expect(rows()).toHaveLength(0);
	});

	// The rows came off the page, not out of the lookup, so a lookup that could
	// not run leaves them on the list and says what is missing from them —
	// rather than throwing away what was already read.
	it.each([
		['no-client-id', /couldn’t find SoundCloud’s API key in the page/],
		['unauthorized', /SoundCloud rejected the lookups/]
	])('adds the tracks but says their links went unchecked (%s)', async (error, message) => {
		panelChrome({ response: answer([track('A', 'B')], { error }) });
		await open();
		await addPage();
		expect(text()).toMatch(/added 1 track from My Playlist, but their links weren’t checked/);
		expect(text()).toMatch(message);
		expect(listedTracks()).toEqual(['/a/b']);
		expect(rows()).toHaveLength(1);
	});

	// The tab went somewhere else between the read that drew the button and the
	// press: the rows are on, and the reason the rest could not be is said.
	it('says so when the page cannot be reached for the lookup', async () => {
		panelChrome({ response: answer([track('A', 'B')]) });
		await open();
		chrome.tabs.sendMessage.mockRejectedValue(new Error('gone'));
		await addPage();
		expect(text()).toMatch(/added 1 track from My Playlist, but their links weren’t checked/);
		expect(text()).toMatch(/can’t reach the page/);
		expect(listedTracks()).toEqual(['/a/b']);
	});

	it('says so when no track on the page could be checked', async () => {
		panelChrome({
			response: answer([track('A', 'B', { details: null }), track('C', 'D', { details: null })], {
				lookupFailed: 2
			})
		});
		await load();
		expect(meta()).toBe('2 tracks · 0 download links · 2 not checked');
		expect(text()).toMatch(/added 2 tracks from My Playlist/);
		expect(text()).toMatch(/couldn’t check 2 tracks — SoundCloud didn’t answer for them\./);
		// They are still rows, because they are still on the list: what the tool
		// does not know about a track is not a reason to hide it.
		expect(sections().map((one) => one.heading)).toEqual(['not checked']);
		expect(sections()[0].hint).toMatch(/nobody has asked SoundCloud about these/);
	});

	// The list refuses rather than dropping its oldest, and says how many it
	// refused. Both reads offer the same tracks, so the one that did not fit is
	// one track, not one from each read. A search that hides every row keeps
	// five thousand of them from being drawn: the list holds them either way.
	it('says how many tracks did not fit when the list is full', async () => {
		panelChrome({
			response: answer([
				track('A', 'One', { details: details({ downloadable: true }) }),
				track('B', 'Two', { details: details({ downloadable: true }) })
			]),
			storage: {
				listed: Array.from({ length: 4999 }, (_, n) => ({
					permalink: `/seed/t${n}`,
					title: `T${n}`,
					artist: 'Seed',
					from: 'Seed',
					at: 1
				}))
			},
			session: { view: { open: 'manager', panels: { manager: { query: 'zzzz' } } } }
		});
		await open();
		await addPage();
		expect(rows()).toHaveLength(0);
		expect(text()).toMatch(/added 1 track from My Playlist\. 1 track wouldn’t fit: the list holds 5000\./);
		const kept = listedTracks();
		expect(kept).toHaveLength(5000);
		expect(kept.at(-1)).toBe('/a/one');
	});

	it('says why when SoundCloud will not describe the tracks at all', async () => {
		// A private playlist: the lookup stopped rather than asking about each row
		// in turn, and the note has to say that rather than leave it at "could not
		// be checked", which would read as a failure to fix.
		panelChrome({
			response: answer([track('A', 'B', { details: null }), track('C', 'D', { details: null })], {
				lookupFailed: 0,
				unseen: true
			})
		});
		await load();
		expect(text()).toMatch(/couldn’t check 2 tracks: SoundCloud won’t describe them to a signed-out visitor/);
		expect(text()).toMatch(/usually that means they’re private/);
	});
});

// A song page offers one track — the one it is about — and it goes onto the
// list the way a page of fifty does. The bar counts it, the button offers it,
// and what a row came in from is the page's own name.
describe('adding a song page', () => {
	const song = (extra = {}) =>
		answer([track('Bicep', 'Glue', { genre: 'Techno', ...extra })], {
			pageType: 'track',
			title: 'Glue',
			curator: 'Bicep'
		});

	it('offers the one track the page is about, and lists it', async () => {
		panelChrome({ response: song() });
		await open();
		expect(text()).toMatch(/Glue/);
		expect(text()).toMatch(/by Bicep · 1 track/);
		expect(clean(addButton())).toBe('add 1');

		await addPage();
		expect(listedTracks()).toEqual(['/bicep/glue']);
		expect(text()).toMatch(/added 1 track from Glue/);
	});

	// A page that renders no title of its own is still a page of a known kind,
	// and that is a better answer on a row than nothing at all.
	it('files a track from an untitled song page under the kind of page it was', async () => {
		panelChrome({
			response: answer([track('Bicep', 'Glue')], { pageType: 'track', title: '', curator: '' })
		});
		await open();
		await addPage();
		expect(text()).toMatch(/added 1 track from a track page/);
		expect(chrome.testStore('local').listed[0].from).toBe('a track page');
	});

	// Adding the song page a track is already on from a playlist is a refresh,
	// as adding any page twice is: one track, and the better of the two answers.
	it('refreshes a track already on the list rather than listing it twice', async () => {
		panelChrome({ response: answer([track('Bicep', 'Glue', { details: null })]) });
		await load();
		expect(listedTracks()).toEqual(['/bicep/glue']);

		chrome.tabs.sendMessage.mockResolvedValue(
			song({ details: details({ permalink: '/bicep/glue', downloadable: true }) })
		);
		await reopenPanel('manager');
		expect(clean(addButton())).toBe('refresh 1');
		await addPage();
		expect(listedTracks()).toEqual(['/bicep/glue']);
		expect(text()).toMatch(/refreshed 1 track from Glue/);
	});

	it('says why a song page whose hero has not drawn yet has nothing to add', async () => {
		panelChrome({ response: answer([], { pageType: 'track', title: '' }) });
		await open();
		expect(text()).toMatch(/no track here yet\. let the page finish loading/);
		expect(addButton()!.disabled).toBe(true);
	});
});

describe('while the lookup is still running', () => {
	// The page's tracks come back in a few milliseconds; their download details
	// take as long as SoundCloud takes, which on a scrolled page is seconds. The
	// panel used to await the second and show a spinner in place of the list
	// until it landed. These are about the list going up on the first answer and
	// filling in on the second, which is the whole of why the popup feels fast.
	//
	// `arrive` settles the details read; `report` pushes a progress message the
	// way the content script does as each answer comes in.
	function slowLookup(tracks: object[]) {
		let deliver: (value: PageReply) => void;

		const lookedUp = new Promise<PageReply>((resolve) => {
			deliver = resolve;
		});

		panelChrome({ response: undefined });
		chrome.tabs.sendMessage.mockImplementation((_id, message) =>
			message.type === 'EXTRACT_TRACKS' ? Promise.resolve(answer(tracks)) : lookedUp
		);

		return {
			arrive: async (finished?: object[]) => {
				deliver(answer(finished ?? tracks));
				await settle();
			},
			report: async (message: Partial<DetailsProgress>) => {
				await chrome.testSendMessage({ type: 'DOWNLOAD_DETAILS_PROGRESS', ...message });
				await settle();
			}
		};
	}

	const PENDING = [track('A', 'One'), track('B', 'Two')];

	it('lists the page’s tracks before any detail has been looked up', async () => {
		slowLookup(PENDING);
		await load();

		// Every track is on screen, named, and says what it is waiting for.
		expect(rows()).toHaveLength(2);
		expect(sections()).toEqual([
			{
				heading: 'not checked',
				count: '(2)',
				hint: 'asking SoundCloud about these.',
				rows: [
					{ permalink: '/a/one', detail: null, buttons: [] },
					{ permalink: '/b/two', detail: null, buttons: [] }
				]
			}
		]);
		// And nothing claims an answer nobody has: no count of tracks without
		// links, no note about tracks that couldn’t check, and no search
		// badges offering somewhere else to look.
		expect(meta()).toBe('2 tracks · 0 download links · 2 not checked');
		expect(text()).not.toMatch(/couldn’t check/);
		expect(text()).not.toMatch(/without/);
	});

	// In list order there is no "not checked" section to wait in, so each row
	// says it for itself — and carries no search badges until SoundCloud answers.
	it('says each row is being checked in list order too', async () => {
		localStorage.setItem('pref:sort', 'page');
		slowLookup(PENDING);
		await load();

		expect(sections()).toEqual([
			{
				heading: null,
				count: null,
				hint: null,
				rows: [
					{ permalink: '/a/one', detail: 'checking…', buttons: [] },
					{ permalink: '/b/two', detail: 'checking…', buttons: [] }
				]
			}
		]);
	});

	it('fills a row in as its answer arrives, without waiting for the rest', async () => {
		const { report } = slowLookup(PENDING);
		await load();

		await report({
			done: 1,
			total: 2,
			resumed: false,
			details: [{ ...DETAILS, permalink: '/a/one', downloadable: true }]
		});

		expect(sections()).toEqual([
			{
				heading: 'soundcloud.com',
				count: '(1)',
				hint: expect.stringContaining('SoundCloud'),
				rows: [{ permalink: '/a/one', detail: null, buttons: ['open the track page to download'] }]
			},
			{
				heading: 'not checked',
				count: '(1)',
				hint: expect.any(String),
				rows: [{ permalink: '/b/two', detail: null, buttons: [] }]
			}
		]);
		expect(meta()).toBe('2 tracks · 1 download link · 1 not checked');

		// The bar says what it is measuring, and how far, in words.
		const bar = document.querySelector('[role="progressbar"]')!;
		expect(bar.getAttribute('aria-label')).toBe('checking download details');
		expect(bar.getAttribute('aria-valuetext')).toBe('1 of 2');
	});

	it('settles into the finished list when the details land', async () => {
		const { arrive } = slowLookup(PENDING);
		await load();

		await arrive([
			track('A', 'One', { details: details({ downloadable: true }) }),
			track('B', 'Two', { details: details({ permalink: '/b/two' }) })
		]);

		// "Not checked" is gone, and the track that turned out to have nothing is
		// said to have nothing.
		expect(sections().map((section) => section.heading)).toEqual(['soundcloud.com', 'no download links']);
		expect(meta()).toBe('2 tracks · 1 download link · 1 without');
	});

	it('draws a screenful first and the rest of a long page after it', async () => {
		// Five hundred rows are the best part of a second of DOM work and the
		// popup shows about eight at a time, so the first paint is a screenful and
		// the rest arrive over the frames after it.
		vi.useFakeTimers();
		const many = Array.from({ length: 500 }, (_, i) => track(`A${i}`, `T${i}`));
		slowLookup(many);
		await load();
		expect(rows()).toHaveLength(60);

		// Growth is frame-bound, with a timeout behind it for a page that is
		// hidden and gets no frames. Under fake timers it is the timeout that
		// drives it, so this is the same number of rounds every run.
		const grown = async () => {
			for (let i = 0; i < 6; i++) {
				await vi.advanceTimersByTimeAsync(60);
				await settle();
			}
		};

		await grown();
		expect(rows()).toHaveLength(500);
		// Every row drawn once: no duplicates, and none left over from the
		// screenful that went up first.
		expect(new Set(rows().map((li) => li.textContent)).size).toBe(500);

		// Filtered down to one row, and the whole list back when that is cleared.
		const search = document.querySelector('[aria-label="search the list"]');
		await type(search, 'T499');
		expect(rows()).toHaveLength(1);
		await type(search, '');
		await grown();
		expect(rows()).toHaveLength(500);
		expect(new Set(rows().map((li) => li.textContent)).size).toBe(500);
	});
});

describe('grouping links by site', () => {
	const TRACKS = [
		track('Hype Artist', 'One', {
			details: details({
				purchaseUrl: 'https://gate.sc?url=https%3A%2F%2Fhypeddit.com%2Ftrack%2Fone&token=t',
				purchaseTitle: 'Free Download'
			})
		}),
		track('SC Artist', 'Two', { details: details({ downloadable: true }) }),
		track('BC Artist', 'Three', {
			details: details({ purchaseUrl: 'https://dani3l.bandcamp.com/track/three' })
		}),
		track('BC Artist', 'Four', {
			details: details({
				downloadable: true,
				hasDownloadsLeft: false,
				purchaseUrl: 'https://www.bandcamp.com/track/four',
				purchaseTitle: 'Buy'
			})
		}),
		track('Drop Artist', 'Five', { details: details({ purchaseUrl: 'https://droploud.com/five' }) }),
		track('UK Artist', 'Six', {
			details: details({ purchaseUrl: 'https://artist.bandcamp.co.uk/six' })
		}),
		track('Odd Artist', 'Seven', { details: details({ purchaseUrl: 'not a url' }) }),
		track('Gate Artist', 'Eight', { details: details({ purchaseUrl: 'https://gate.sc?token=only' }) }),
		track('Page Artist', 'Nine', {
			domPurchaseTitle: 'Stream / Download',
			details: details({
				downloadable: null,
				hasDownloadsLeft: null,
				purchaseUrl: 'https://hypeddit.com/nine',
				partial: true
			})
		}),
		track('Nolink Artist', 'Ten', { details: details() }),
		track('', 'Eleven', { permalink: '/someone/eleven', details: details() }),
		track('Unknown Artist', 'Twelve', { details: null }),
		track('Nopermalink Artist', 'Thirteen', {
			permalink: '',
			details: details({ downloadable: true })
		})
	];

	beforeEach(async () => {
		panelChrome({
			response: answer(TRACKS, { lookupFailed: 0, skippedPlaylists: 1 }),
			storage: { downloaded: { '/scartist/two': ISO, '/elsewhere/x': ISO } }
		});
		await load();
	});

	it('lists soundcloud.com first, then sites by size, then alphabetically, with the tracks without links last', () => {
		expect(sections().map((s) => [s.heading, s.count])).toEqual([
			['soundcloud.com', '(2)'],
			['bandcamp.com', '(2)'],
			['hypeddit.com', '(2)'],
			['bandcamp.co.uk', '(1)'],
			['droploud.com', '(1)'],
			['gate.sc', '(1)'],
			['no download links', '(2)'],
			// Two nobody has an answer about: one whose details never arrived, and
			// one whose purchase link is not a URL — so its SoundCloud download is
			// the only half of the question that was ever answered.
			['not checked', '(2)']
		]);
	});

	it('summarises the list, and says what the page offered', () => {
		// Twelve, not thirteen: the track with no permalink is not something the
		// list can hold, since there would be nothing to tell it from another.
		expect(meta()).toBe('12 tracks · 9 download links · 2 without · 2 not checked');
		// What the page itself was is said by the bar that offered it, which is
		// where a fact about one page belongs once the list spans several.
		expect(text()).toMatch(/by Curator · 13 tracks · 12 already here · 1 liked playlist skipped/);
		expect(text()).toMatch(
			/couldn’t check 1 track\. 1 track: purchase link read off the page, SoundCloud download unknown\./
		);
	});

	it('describes each link: one static download button, the real host, and SoundCloud’s download state', () => {
		const [soundcloud, bandcamp, hypeddit, uk, , gate, none] = sections();
		expect(soundcloud.hint).toMatch(/^SoundCloud downloads live in each track’s More menu/);
		expect(soundcloud.rows.map((r) => [r.permalink, r.detail, r.buttons])).toEqual([
			['/scartist/two', null, ['open the track page to download']],
			['/bcartist/four', 'no downloads left', ['open the track page to download (no downloads left)']]
		]);
		expect(isMarked('/scartist/two')).toBe(true);
		expect(isMarked('/bcartist/four')).toBe(false);

		expect(bandcamp.hint).toBe(null);
		expect(bandcamp.rows.map((r) => [r.detail, r.buttons])).toEqual([
			['dani3l.bandcamp.com', ['download from dani3l.bandcamp.com']],
			[null, ['download from bandcamp.com']]
		]);
		// Both say the same thing, though the page called the second one
		// "Stream / Download": the button names what it does, not what the
		// uploader wrote on theirs (that is in the tooltip — see below).
		expect(hypeddit.rows.map((r) => [r.permalink, r.buttons])).toEqual([
			['/hypeartist/one', ['download from hypeddit.com']],
			['/pageartist/nine', ['download from hypeddit.com']]
		]);
		expect(openButtons('/pageartist/nine')[0].title).toMatch(/^“Stream \/ Download” — opens /);
		expect(uk.rows[0].detail).toBe('artist.bandcamp.co.uk');
		expect(gate.rows[0].detail).toBe(null);
		expect(none.hint).toBe(
			'no SoundCloud download and no purchase link. the badges search the artist on Beatport, Bandcamp, ZIPDJ, MyMP3Pool.'
		);
		expect(none.rows.map((r) => r.buttons)).toEqual([
			['BP', 'bc', 'Z', 'M'],
			['BP', 'bc', 'Z', 'M']
		]);

		const badges = (permalink: string) =>
			[...rowFor(permalink).querySelectorAll('button')].filter((b) =>
				(b.getAttribute('aria-label') ?? '').includes(': search ')
			);

		expect(badges('/nolinkartist/ten').map((b) => b.disabled)).toEqual([false, false, false, false]);
		expect(badges('/someone/eleven').map((b) => b.disabled)).toEqual([true, true, true, true]);
		expect(badges('/nolinkartist/ten')[0].title).toBe('search Beatport for “Nolink Artist”');
		// Named by the letters on it, which is what a voice command says; the words searched for are the description.
		expect(badges('/nolinkartist/ten')[0].getAttribute('aria-label')).toBe('BP: search Beatport');
	});

	it('explains what each download button does', () => {
		expect(openButtons('/scartist/two')[0].title).toBe('opens the track page — grab it from More → Download file');
		// The uploader's own wording for the link is here rather than written
		// across the button, which is one static icon on every row.
		expect(openButtons('/hypeartist/one')[0].title).toBe(
			'“Free Download” — opens hypeddit.com, and marks this downloaded'
		);
		expect(rowFor('/scartist/two').querySelector('img')!.src).toMatch(/artworks-two-t120x120\.jpg$/);
	});

	it('words the summary and notes for a Likes page with several skipped playlists and partial answers', async () => {
		document.body.innerHTML = '';
		panelChrome({
			response: answer(
				[
					track('A', 'One', {
						details: details({
							downloadable: null,
							hasDownloadsLeft: null,
							purchaseUrl: 'https://hypeddit.com/one',
							partial: true
						})
					}),
					track('B', 'Two', {
						details: details({
							downloadable: null,
							hasDownloadsLeft: null,
							purchaseUrl: 'https://hypeddit.com/two',
							partial: true
						})
					}),
					track('C', 'Three', { details: null }),
					track('D', 'Four', { details: null })
				],
				{ pageType: 'likes', title: '', curator: '', skippedPlaylists: 2, lookupFailed: 0 }
			)
		});
		await load();
		expect(meta()).toBe('4 tracks · 2 download links · 2 not checked');
		expect(text()).toMatch(/2 liked playlists skipped/);
		expect(text()).toMatch(
			/couldn’t check 2 tracks\. 2 tracks: purchase link read off the page, SoundCloud download unknown\./
		);
		expect(sections().map((s) => [s.heading, s.count])).toEqual([
			['hypeddit.com', '(2)'],
			['not checked', '(2)']
		]);
		// A likes page with no heading of its own: the tracks are still filed
		// under something a person can read.
		expect(text()).toMatch(/added 4 tracks from a likes page/);
	});

	it('shows a placeholder for tracks with no cover anywhere', async () => {
		document.body.innerHTML = '';
		panelChrome({
			response: answer([track('A', 'B', { artworkUrl: '', details: details({ downloadable: true }) })])
		});
		await load();
		expect(rowFor('/a/b').querySelector('img')).toBe(null);
		expect(rowFor('/a/b').querySelector('[title="no cover art"]')).not.toBe(null);
		expect(meta()).toBe('1 track · 1 download link');
		expect(text()).not.toMatch(/couldn’t check/);
	});

	// A page lazy-loads its thumbnails as they scroll into view, so a list read
	// without scrolling through it hands over rows with no artwork at all. What
	// SoundCloud said when it was asked about the track names one anyway.
	it('shows the cover the details name when the row never loaded one', async () => {
		document.body.innerHTML = '';
		const fromApi = 'https://i1.sndcdn.com/artworks-b-large.jpg';
		panelChrome({
			response: answer([
				track('A', 'B', { artworkUrl: '', details: details({ downloadable: true, artworkUrl: fromApi }) })
			])
		});
		await load();
		expect(rowFor('/a/b').querySelector('img')!.src).toBe(fromApi);
		expect(rowFor('/a/b').querySelector('[title="no cover art"]')).toBe(null);
	});

	// Adding a page again is how a list is brought up to date — and a second
	// reading of it, from the top, renders far fewer thumbnails than the first
	// scrolled-through one did. "The row showed none" must not take the cover
	// off a track that has one.
	it('keeps the cover when the page is added again without its thumbnails', async () => {
		document.body.innerHTML = '';
		panelChrome({ response: answer([track('A', 'One', { details: details({ downloadable: true }) })]) });
		await load();
		expect(rowFor('/a/one').querySelector('img')!.src).toMatch(/artworks-one-t120x120\.jpg$/);

		chrome.tabs.sendMessage.mockResolvedValue(
			answer([track('A', 'One', { artworkUrl: '', details: details({ downloadable: true }) })])
		);
		await reopenPanel('manager');
		await addPage();
		expect(text()).toMatch(/refreshed 1 track/);
		expect(rowFor('/a/one').querySelector('img')!.src).toMatch(/artworks-one-t120x120\.jpg$/);
	});
});

// A playlist can contain the same track twice, and the permalink is the
// list's identity for a track — so adding such a page puts it on the list
// once, however many times the page showed it.
describe('a track the page lists twice', () => {
	const TWICE = [
		track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) }),
		track('B', 'Two', { details: details({ downloadable: true }) }),
		track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) })
	];

	it('goes onto the list once', async () => {
		panelChrome({ response: answer(TWICE) });
		await load();
		expect(listedTracks()).toEqual(['/a/one', '/b/two']);
		expect(sections()[0].rows.map((r) => r.permalink)).toEqual(['/b/two']);
	});

	it('is one row per link it offers, grouped by site', async () => {
		panelChrome({ response: answer(TWICE) });
		await load();
		expect(sections().map((s) => [s.heading, s.count])).toEqual([
			['soundcloud.com', '(1)'],
			['hypeddit.com', '(1)']
		]);
		expect(document.querySelectorAll('li[data-permalink="/a/one"]')).toHaveLength(1);
	});

	it('is one row in list order', async () => {
		localStorage.setItem('pref:sort', 'page');
		panelChrome({ response: answer(TWICE) });
		await load();
		expect(sections()[0].rows.map((r) => r.permalink)).toEqual(['/a/one', '/b/two']);
	});

	// A track *can* still be drawn twice: the by-site layout files a SoundCloud
	// download and a purchase link in two sections. One tick, one mark, both
	// rows — the same rule as ever, now that the duplicate is not what causes it.
	it('marks every row of a track at once when its links put it in two sections', async () => {
		panelChrome({
			response: answer([
				track('A', 'One', {
					details: details({ downloadable: true, purchaseUrl: 'https://hypeddit.com/one' })
				})
			])
		});
		await load();
		expect(document.querySelectorAll('li[data-permalink="/a/one"]')).toHaveLength(2);
		expect(meta()).toMatch(/1 track · 2 download links/);
		await click(markButton('/a/one', 0));
		expect(isMarked('/a/one', 0)).toBe(true);
		expect(isMarked('/a/one', 1)).toBe(true);
		expect(clean(clearButton())).toBe('unmark (1)');
	});
});

describe('opening links', () => {
	beforeEach(async () => {
		panelChrome({
			response: answer([
				track('Hype Artist', 'One', {
					details: details({ purchaseUrl: 'https://hypeddit.com/one' })
				}),
				track('SC Artist', 'Two', { details: details({ downloadable: true }) }),
				track('Nolink & Ünï', 'Three', { permalink: '/nolink/three', details: details() })
			])
		});
		await load();
	});

	it('opens an external link in a background tab and queues the track', async () => {
		await click(openButtons('/hypeartist/one')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: 'https://hypeddit.com/one',
			active: false
		});
		// Opening a link is not a download: the track waits in the session queue,
		// and nothing is marked until a batch of them is.
		expect(queued()).toEqual(['/hypeartist/one']);
		expect(isQueued('/hypeartist/one')).toBe(true);
		expect(chrome.testStore().downloaded).toBeUndefined();
		expect(isMarked('/hypeartist/one')).toBe(false);
		expect(clean(clearButton())).toBe('unmark');
	});

	it('opens a SoundCloud track page without marking it, in the foreground when asked', async () => {
		await click(document.getElementById('background'));
		expect(localStorage.getItem('pref:background')).toBe('0');
		await click(openButtons('/scartist/two')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: 'https://soundcloud.com/scartist/two',
			active: true
		});
		expect(chrome.testStore().downloaded).toBeUndefined();
	});

	it('opens artist searches with the name encoded, marking nothing', async () => {
		for (const badge of openButtons('/nolink/three')) await click(badge);
		const q = encodeURIComponent('Nolink & Ünï');
		expect(chrome.tabs.create.mock.calls.map((c) => c[0].url)).toEqual([
			`https://www.beatport.com/search?q=${q}`,
			`https://bandcamp.com/search?q=${q}`,
			`https://www.zipdj.com/app/search?q=${q}`,
			`https://mp3poolonline.com/results/artists?search=${q}`
		]);
		expect(chrome.testStore().downloaded).toBeUndefined();
	});
});

// Which shops a link-less row offers is a setting rather than a fact about the
// list (src/lib/search-sites.json holds the sites, the settings sheet holds
// the switches), so the badges a row carries are whatever is switched on —
// down to none of them, which is a row that says so rather than one that looks
// unfinished.
describe('the search badges a row carries', () => {
	const NO_LINKS = [track('Nolink Artist', 'One', { permalink: '/nolink/one', details: details() })];

	const openWith = async (off: string[] = []) => {
		for (const id of off) localStorage.setItem(`pref:search:${id}`, '0');
		panelChrome({ response: answer(NO_LINKS) });
		await load();
	};

	const badges = () => openButtons('/nolink/one').map((b) => clean(b));
	const noLinksHint = () => sections().at(-1)!.hint;

	it('is every site, in the file’s order, until somebody says otherwise', async () => {
		await openWith();
		expect(badges()).toEqual(['BP', 'bc', 'Z', 'M']);
		expect(noLinksHint()).toBe(
			'no SoundCloud download and no purchase link. the badges search the artist on Beatport, ' +
				'Bandcamp, ZIPDJ, MyMP3Pool.'
		);
	});

	it('leaves out the ones switched off, and stops naming them', async () => {
		await openWith(['bandcamp', 'mp3pool']);
		expect(badges()).toEqual(['BP', 'Z']);
		expect(noLinksHint()).toBe(
			'no SoundCloud download and no purchase link. the badges search the artist on Beatport, ZIPDJ.'
		);
		// And the one badge left still opens the one search.
		await click(openButtons('/nolink/one')[0]);
		expect(chrome.tabs.create.mock.calls.map((c) => c[0].url)).toEqual([
			`https://www.beatport.com/search?q=${encodeURIComponent('Nolink Artist')}`
		]);
	});

	it('says so where every one of them is off, rather than drawing nothing', async () => {
		await openWith(['beatport', 'bandcamp', 'zipdj', 'mp3pool']);
		expect(badges()).toEqual([]);
		expect(noLinksHint()).toBe(
			'no SoundCloud download and no purchase link, and no search badges switched on to try elsewhere with.'
		);
	});

	it('says as much in list order, where the same rows carry them', async () => {
		localStorage.setItem('pref:sort', 'page');
		await openWith(['mp3pool']);
		expect(sections()[0].hint).toBe(
			'tracks with no link carry badges that search the artist on Beatport, Bandcamp, ZIPDJ.'
		);
	});

	it('points at settings in list order when every one is off', async () => {
		localStorage.setItem('pref:sort', 'page');
		await openWith(['beatport', 'bandcamp', 'zipdj', 'mp3pool']);
		expect(badges()).toEqual([]);
		expect(sections()[0].hint).toBe('tracks with no link carry no search badges — turn some on in settings.');
	});
});

describe('marks', () => {
	const ROWS = [
		track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) }),
		track('B', 'Two', {
			details: details({ downloadable: true, purchaseUrl: 'https://hypeddit.com/two' })
		}),
		track('C', 'Three', { details: details() }),
		track('D', 'Four', { details: null })
	];

	it('toggles a track’s mark by hand, on every row it has', async () => {
		panelChrome({ response: answer(ROWS) });
		await load();
		expect(clearButton()!.disabled).toBe(true);
		expect(clean(clearButton())).toBe('unmark');

		await click(markButton('/b/two', 0));
		expect(chrome.testStore().downloaded).toEqual({ '/b/two': ISO });
		expect(isMarked('/b/two', 0)).toBe(true);
		expect(isMarked('/b/two', 1)).toBe(true);
		expect(clearButton()!.disabled).toBe(false);
		expect(clean(clearButton())).toBe('unmark (1)');

		await click(markButton('/b/two', 1));
		expect(chrome.testStore().downloaded).toEqual({});
		expect(isMarked('/b/two', 0)).toBe(false);
		expect(clearButton()!.disabled).toBe(true);
	});

	it('clears every mark only when asked twice, and forgets the first click after a moment', async () => {
		vi.useFakeTimers();
		panelChrome({
			response: answer(ROWS),
			storage: { downloaded: { '/a/one': ISO, '/elsewhere/x': ISO } }
		});
		await load();
		expect(clean(clearButton())).toBe('unmark (2)');
		expect(isMarked('/a/one')).toBe(true);

		await click(clearButton());
		expect(clean(clearButton())).toBe('unmark 2?');
		await vi.advanceTimersByTimeAsync(4000);
		await settle();
		expect(clean(clearButton())).toBe('unmark (2)');

		await click(clearButton());
		await click(clearButton());
		expect(chrome.testStore().downloaded).toEqual({});
		expect(clean(clearButton())).toBe('unmark');
		expect(clearButton()!.disabled).toBe(true);
		expect(isMarked('/a/one')).toBe(false);
		expect(text()).toMatch(/every mark cleared/);

		await vi.advanceTimersByTimeAsync(4000);
		await settle();
		expect(clean(clearButton())).toBe('unmark');
	});

	it('words the confirmation for a single mark', async () => {
		panelChrome({ response: answer(ROWS), storage: { downloaded: { '/a/one': ISO } } });
		await load();
		await click(clearButton());
		expect(clean(clearButton())).toBe('unmark 1?');
	});

	it('follows marks made elsewhere, and ignores other storage changes', async () => {
		panelChrome({ response: answer(ROWS) });
		await load();
		chrome.testEmitStorageChange({ downloaded: { newValue: { '/c/three': ISO } } });
		await settle();
		expect(isMarked('/c/three')).toBe(true);
		expect(clean(clearButton())).toBe('unmark (1)');

		// Marks live in the local area; a `downloaded` key anywhere else is not
		// them, however much it looks like them.
		chrome.testEmitStorageChange({ downloaded: { newValue: { '/a/one': ISO } } }, 'sync');
		chrome.testEmitStorageChange({ details: { newValue: {} } });
		await settle();
		expect(isMarked('/a/one')).toBe(false);
		expect(isMarked('/c/three')).toBe(true);
		expect(clean(clearButton())).toBe('unmark (1)');

		chrome.testEmitStorageChange({ downloaded: { newValue: undefined } });
		await settle();
		expect(isMarked('/c/three')).toBe(false);
		expect(clearButton()!.disabled).toBe(true);
	});

	it('starts without marks when the extension’s storage cannot be read', async () => {
		panelChrome({ response: answer(ROWS) });
		chrome.storage.local.get.mockRejectedValue(new Error('Extension context invalidated'));
		await load();
		expect(clearButton()!.disabled).toBe(true);
		expect(rows().length).toBeGreaterThan(0);

		// Nothing was read, so nothing is followed either: a mark made elsewhere
		// does not reach a panel whose store never started.
		chrome.testEmitStorageChange({ downloaded: { newValue: { '/a/one': ISO } } });
		await settle();
		expect(isMarked('/a/one')).toBe(false);
	});
});

describe('the session queue', () => {
	const ROWS = [
		track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) }),
		track('B', 'Two', { details: details({ downloadable: true }) }),
		track('C', 'Three', { details: details() }),
		track('D', 'Four', { details: null })
	];

	it('queues a track by hand, and takes it out again', async () => {
		panelChrome({ response: answer(ROWS) });
		await load();
		expect(isQueued('/a/one')).toBe(false);

		await click(queueButton('/a/one'));
		expect(queued()).toEqual(['/a/one']);
		expect(isQueued('/a/one')).toBe(true);
		// Queueing is not marking: nothing has been downloaded yet.
		expect(isMarked('/a/one')).toBe(false);

		await click(queueButton('/a/one'));
		expect(queued()).toEqual([]);
		expect(isQueued('/a/one')).toBe(false);
	});

	it('keeps what it queues: title, artist, artwork and the download details', async () => {
		panelChrome({ response: answer(ROWS) });
		await load();
		await click(queueButton('/a/one'));
		// Enough for the tools that read the queue to list the track, show its
		// artwork and find its gate, without the page it came from — plus the
		// three things only that page could have said, which come through as
		// "nobody knows" for a row that said none of them.
		expect(chrome.testStore('session').queue).toEqual([
			{
				permalink: '/a/one',
				title: 'One',
				artist: 'A',
				artworkUrl: 'https://i1.sndcdn.com/artworks-one-t120x120.jpg',
				details: details({ purchaseUrl: 'https://hypeddit.com/one' }),
				genre: null,
				liked: null,
				reposted: null
			}
		]);
	});

	it('shows what another popup window queued', async () => {
		panelChrome({ response: answer(ROWS) });
		await load();
		chrome.testEmitStorageChange({ queue: { newValue: [{ permalink: '/c/three' }] } }, 'session');
		await settle();
		expect(isQueued('/c/three')).toBe(true);
		expect(isQueued('/a/one')).toBe(false);
	});
});

// Selecting is its own step: tick the tracks, then put a marker on the lot.
// The row buttons no longer know anything about batches — see
// src/lib/selection.svelte.js for why that changed.
describe('selecting a batch', () => {
	// Every one of them looked up, and `start` sorts the list in list order, so
	// each track is one row and one tick box.
	const ROWS = [
		track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) }),
		track('B', 'Two', { details: details({ downloadable: true }) }),
		track('C', 'Three', { details: details() }),
		track('D', 'Four', { details: details() })
	];

	const marks = () => Object.keys(chrome.testStore().downloaded).sort();

	const start = async (options = {}) => {
		localStorage.setItem('pref:sort', 'page');
		panelChrome({ response: answer(ROWS), ...options });
		await load();
		await click(selectButton());
	};

	it('shows no tick boxes until Select is on, and takes them away again', async () => {
		localStorage.setItem('pref:sort', 'page');
		panelChrome({ response: answer(ROWS) });
		await load();
		expect(selectButton()!.getAttribute('aria-pressed')).toBe('false');
		expect(tickBox('/a/one')).toBe(null);
		expect(batchButton('queue')).toBe(undefined);

		await click(selectButton());
		expect(selectButton()!.getAttribute('aria-pressed')).toBe('true');
		expect(tickBox('/a/one')).not.toBe(null);
		expect(text()).toMatch(/nothing ticked/);

		await click(selectButton());
		expect(tickBox('/a/one')).toBe(null);
	});

	it('ticks a track and unticks it, and says how many are ticked', async () => {
		await start();
		await tick('/a/one');
		expect(isTicked('/a/one')).toBe(true);
		expect(text()).toMatch(/1 track ticked/);

		await tick('/c/three');
		expect(text()).toMatch(/2 tracks ticked/);

		await tick('/a/one');
		expect(isTicked('/a/one')).toBe(false);
		expect(text()).toMatch(/1 track ticked/);
	});

	it('takes in every track between the last one ticked and a Shift-click', async () => {
		await start();
		await tick('/a/one');
		await click(tickBox('/c/three'), { shiftKey: true });
		expect(['/a/one', '/b/two', '/c/three'].map((p) => isTicked(p))).toEqual([true, true, true]);
		expect(isTicked('/d/four')).toBe(false);

		// Upwards reads the same, and the row Shift-clicked becomes what the next
		// one reaches from.
		await tick('/d/four');
		await click(tickBox('/c/three'), { shiftKey: true });
		expect(isTicked('/d/four')).toBe(true);
	});

	it('ticks the one row when a Shift-click has nothing to reach from', async () => {
		await start();
		await click(tickBox('/b/two'), { shiftKey: true });
		expect(text()).toMatch(/1 track ticked/);
		expect(isTicked('/b/two')).toBe(true);
	});

	it('ticks every track shown, and unticks them all again', async () => {
		await start();
		await click(selectAllBox());
		expect(['/a/one', '/b/two', '/c/three', '/d/four'].map((p) => isTicked(p))).toEqual([true, true, true, true]);
		expect(text()).toMatch(/4 tracks ticked/);

		await click(selectAllBox());
		expect(text()).toMatch(/nothing ticked/);

		await tick('/a/one');
		await click(button('untick all'));
		expect(text()).toMatch(/nothing ticked/);
	});

	it('ticks only what the filter leaves showing', async () => {
		await start();
		await type(document.querySelector('[aria-label="search the list"]'), 'two');
		await click(selectAllBox());
		expect(text()).toMatch(/1 track ticked/);
		await click(batchButton('queue'));
		expect(queued()).toEqual(['/b/two']);
	});

	// --- the tab in front ------------------------------------------------------
	//
	// The list spans every page it was built from, and the one somebody is
	// looking at is rarely all of it: "everything on this page" is a batch of
	// its own, and the box at the left of the bar cannot say it.
	const ELSEWHERE = {
		permalink: '/old/one',
		title: 'Old',
		artist: 'Elsewhere',
		from: 'Another Playlist',
		details: details({ permalink: '/old/one', downloadable: true })
	};

	it('ticks what the tab is showing, and leaves the rest of the list alone', async () => {
		await start({ storage: { listed: [ELSEWHERE] } });
		expect(clean(pageTickButton())).toBe('tick this page (4)');
		expect(pageTickButton()!.title).toMatch(/ticks the 4 tracks showing from My Playlist/);

		await click(pageTickButton());
		expect(text()).toMatch(/4 tracks ticked/);
		expect(['/a/one', '/b/two', '/c/three', '/d/four'].map((p) => isTicked(p))).toEqual([true, true, true, true]);
		// The track that came in from another page is not on this one.
		expect(isTicked('/old/one')).toBe(false);

		await click(batchButton('mark'));
		expect(marks()).toEqual(['/a/one', '/b/two', '/c/three', '/d/four']);
	});

	it('unticks the page again, and only the page', async () => {
		await start({ storage: { listed: [ELSEWHERE] } });
		await tick('/old/one');
		await click(pageTickButton());
		expect(text()).toMatch(/5 tracks ticked/);

		// Every row of the page is ticked now, so the button turns into the one
		// that takes them back off — leaving what was ticked elsewhere.
		expect(clean(pageTickButton())).toBe('untick this page (4)');
		await click(pageTickButton());
		expect(text()).toMatch(/1 track ticked/);
		expect(isTicked('/old/one')).toBe(true);
	});

	it('ticks only the rows of the page the filter leaves showing', async () => {
		await start({ storage: { listed: [ELSEWHERE] } });
		await type(document.querySelector('[aria-label="search the list"]'), 'two');
		expect(clean(pageTickButton())).toBe('tick this page (1)');
		await click(pageTickButton());
		expect(text()).toMatch(/1 track ticked/);
		await click(batchButton('queue'));
		expect(queued()).toEqual(['/b/two']);
	});

	it('says why it can do nothing, with the page hidden or never added', async () => {
		await start({ storage: { listed: [ELSEWHERE] } });
		await type(document.querySelector('[aria-label="search the list"]'), 'zzzzzz');
		expect(pageTickButton()!.disabled).toBe(true);
		expect(pageTickButton()!.title).toMatch(/hidden by the filter/);

		// A tab with no playlist on it has nothing to offer at all, and says the
		// same thing the add bar above it does.
		localStorage.setItem('pref:sort', 'page');
		panelChrome({ tab: null, storage: { listed: [ELSEWHERE] } });
		await open();
		await click(selectButton());
		expect(clean(pageTickButton())).toBe('tick this page');
		expect(pageTickButton()!.disabled).toBe(true);
		expect(pageTickButton()!.title).toMatch(/not a SoundCloud playlist, likes or track page/);
	});

	it('queues the ticked tracks in list order, in one write, and takes them out again', async () => {
		await start();
		await tick('/c/three', '/a/one');
		await click(batchButton('queue'));
		// The order the page has them in, not the order they were ticked in.
		expect(queued()).toEqual(['/a/one', '/c/three']);
		// One write for the pair, whatever else the session holds (the ticks are
		// kept there too).
		expect(queueWrites()).toBe(1);
		expect(text()).toMatch(/queued 2 tracks/);
		// The ticks stay: the same batch is often the one to mark next.
		expect(isTicked('/a/one')).toBe(true);

		await click(batchButton('unqueue'));
		expect(queued()).toEqual([]);
		expect(text()).toMatch(/unqueued 2 tracks/);
	});

	it('counts only the tracks the queue really took', async () => {
		await start();
		await tick('/a/one');
		await click(batchButton('queue'));
		await tick('/b/two');
		await click(batchButton('queue'));
		expect(queued()).toEqual(['/a/one', '/b/two']);
		// A is queued already, so only B was added. The first press's toast can
		// still be up, and says the same, so what matters is that none says two.
		expect(text()).toMatch(/queued 1 track/);
		expect(text()).not.toMatch(/queued 2 tracks/);
	});

	it('marks the ticked tracks in one write, and unmarks them again', async () => {
		await start();
		await click(selectAllBox());
		// Counted from here: putting the page on the list is a write of its own,
		// and what this is about is the marker costing one however many are ticked.
		chrome.storage.local.set.mockClear();
		await click(batchButton('mark'));
		expect(marks()).toEqual(['/a/one', '/b/two', '/c/three', '/d/four']);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(text()).toMatch(/marked 4 tracks downloaded/);
		expect(isMarked('/b/two')).toBe(true);
		expect(clean(clearButton())).toBe('unmark (4)');

		await click(batchButton('unmark'));
		expect(marks()).toEqual([]);
		expect(text()).toMatch(/unmarked 4 tracks/);
		expect(isMarked('/b/two')).toBe(false);
	});

	// The one action that takes rows away rather than putting a marker on them,
	// which is why it also unticks: a selection of rows that are gone is not a
	// selection.
	it('takes the ticked tracks off the list, and unticks them with them', async () => {
		await start();
		await tick('/a/one', '/c/three');
		chrome.storage.local.set.mockClear();
		await click(batchButton('remove'));

		expect(listedTracks()).toEqual(['/b/two', '/d/four']);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(text()).toMatch(/took 2 tracks off the list/);
		expect(text()).toMatch(/nothing ticked/);
		// The marks and the queue are not the list, and are left alone.
		expect(chrome.testStore().downloaded).toBeUndefined();
		expect(queued()).toEqual([]);
	});

	it('puts two markers on the one batch', async () => {
		await start();
		await tick('/a/one', '/b/two');
		await click(batchButton('queue'));
		await click(batchButton('mark'));
		expect(queued()).toEqual(['/a/one', '/b/two']);
		expect(marks()).toEqual(['/a/one', '/b/two']);
	});

	it('does nothing with nothing ticked', async () => {
		await start();

		for (const label of ['queue', 'unqueue', 'mark', 'unmark']) {
			expect(batchButton(label)!.disabled).toBe(true);
		}

		expect(button('untick all')!.disabled).toBe(true);
	});

	it('keeps the ticks when the popup closes and opens again, and drops them on Done', async () => {
		await start();
		await tick('/a/one', '/b/two');

		await reopenPanel('manager');
		expect(selectButton()!.getAttribute('aria-pressed')).toBe('true');
		expect(text()).toMatch(/2 tracks ticked/);
		expect(isTicked('/a/one')).toBe(true);

		// Done puts the tick boxes away, and the ticks with them.
		await click(doneButton());
		await click(selectButton());
		expect(text()).toMatch(/nothing ticked/);
	});

	it('ticks a track the page lists twice once, on both of its rows', async () => {
		localStorage.setItem('pref:sort', 'site');
		panelChrome({
			response: answer([
				track('B', 'Two', {
					details: details({ downloadable: true, purchaseUrl: 'https://hypeddit.com/two' })
				})
			])
		});
		await load();
		await click(selectButton());
		await click(tickBox('/b/two', 0));
		expect(isTicked('/b/two', 1)).toBe(true);
		expect(text()).toMatch(/1 track ticked/);

		await click(batchButton('queue'));
		expect(queued()).toEqual(['/b/two']);
	});
});

describe('preferences', () => {
	it('restores the background-tab option and remembers a change', async () => {
		localStorage.setItem('pref:background', '0');
		panelChrome({ response: answer([track('A', 'One', { details: details() })]) });
		await load();
		const background = document.getElementById('background')!;
		expect(background.getAttribute('aria-checked')).toBe('false');
		// The hint is a tooltip under the pointer, and the switch's description for a screen reader.
		expect(document.getElementById(background.getAttribute('aria-describedby')!)?.textContent).toMatch(
			/opens links behind the popup/
		);

		await click(background);
		expect(localStorage.getItem('pref:background')).toBe('1');
	});

	it('offers both orders by name, and re-sorts the list on the one picked', async () => {
		panelChrome({
			response: answer([
				track('A', 'One', {
					details: details({ downloadable: true, purchaseUrl: 'https://hypeddit.com/one' })
				})
			])
		});
		await load();
		expect((await openSelect('sort order')).map((option) => option.textContent.trim())).toEqual([
			'by site',
			'list order'
		]);
		// By site, a track with two links is a row under each of them.
		expect(sections().map((section) => section.heading)).toEqual(['soundcloud.com', 'hypeddit.com']);

		await chooseOption('sort order', 'list order');
		expect(localStorage.getItem('pref:sort')).toBe('page');
		// In list order it is the one row the list has, with both links on it.
		expect(sections()).toHaveLength(1);
		expect(sections()[0].heading).toBe(null);

		await chooseOption('sort order', 'by site');
		expect(localStorage.getItem('pref:sort')).toBe('site');
		expect(sections()).toHaveLength(2);
	});

	it('copes when localStorage refuses to answer', async () => {
		vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
			throw new Error('SecurityError');
		});
		vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
			throw new Error('QuotaExceededError');
		});
		panelChrome({ response: answer([track('A', 'One', { details: details() })]) });
		await load();
		expect(document.getElementById('background')!.getAttribute('aria-checked')).toBe('true');
		await click(document.getElementById('background'));
		expect(document.getElementById('background')!.getAttribute('aria-checked')).toBe('false');
	});
});

describe('list order', () => {
	const TRACKS = [
		track('Hype Artist', 'One', {
			details: details({
				purchaseUrl: 'https://gate.sc?url=https%3A%2F%2Fhypeddit.com%2Ftrack%2Fone&token=t',
				purchaseTitle: 'Free Download'
			})
		}),
		track('SC Artist', 'Two', { details: details({ downloadable: true }) }),
		track('BC Artist', 'Four', {
			details: details({
				downloadable: true,
				hasDownloadsLeft: false,
				purchaseUrl: 'https://www.bandcamp.com/track/four',
				purchaseTitle: 'Buy'
			})
		}),
		track('Nolink Artist', 'Ten', { details: details() }),
		track('Unknown Artist', 'Twelve', { details: null })
	];

	beforeEach(() => localStorage.setItem('pref:sort', 'page'));

	it('lists every track once, with all of its links, under one heading-less section', async () => {
		panelChrome({ response: answer(TRACKS), storage: { downloaded: { '/scartist/two': ISO } } });
		await load();

		const [only] = sections();
		expect(sections()).toHaveLength(1);
		expect(only.heading).toBe(null);
		expect(only.hint).toBe(
			'SoundCloud downloads live in each track’s More menu (More → Download file). using it ' +
				'marks the track downloaded. tracks with no link carry badges that search the artist ' +
				'on Beatport, Bandcamp, ZIPDJ, MyMP3Pool.'
		);
		expect(only.rows.map((r) => [r.permalink, r.detail, r.buttons])).toEqual([
			['/hypeartist/one', 'hypeddit.com', ['download from hypeddit.com']],
			['/scartist/two', 'soundcloud.com', ['open the track page to download']],
			// Both links, but only one of them leading: the SoundCloud download has
			// nothing left to give, so the purchase link takes the button and the
			// detail line reads in the same order.
			[
				'/bcartist/four',
				'bandcamp.com · soundcloud.com (no downloads left)',
				['download from bandcamp.com', 'also — open the track page to download (no downloads left)']
			],
			['/nolinkartist/ten', 'no download links', ['BP', 'bc', 'Z', 'M']],
			// On the list and never checked — which the row says rather than
			// leaving it to look like a track known to have nothing.
			['/unknownartist/twelve', 'not checked', ['BP', 'bc', 'Z', 'M']]
		]);
		expect(isMarked('/scartist/two')).toBe(true);
	});

	it('opens both of a track’s links from its one row', async () => {
		panelChrome({ response: answer(TRACKS) });
		await load();
		const [buy, openTrack] = openButtons('/bcartist/four');
		await click(buy);
		await click(openTrack);
		expect(chrome.tabs.create.mock.calls.map((c) => c[0].url)).toEqual([
			'https://www.bandcamp.com/track/four',
			'https://soundcloud.com/bcartist/four'
		]);
		// Both links belong to the one track, which is queued once.
		expect(queued()).toEqual(['/bcartist/four']);
		expect(chrome.testStore().downloaded).toBeUndefined();
	});

	it('leads with the SoundCloud download while it still has one to give', async () => {
		panelChrome({
			response: answer([
				track('Both Artist', 'Five', {
					details: details({
						downloadable: true,
						purchaseUrl: 'https://hypeddit.com/track/five',
						purchaseTitle: 'Free Download'
					})
				})
			])
		});
		await load();
		expect(sections()[0].rows[0]).toMatchObject({
			detail: 'soundcloud.com · hypeddit.com',
			buttons: ['open the track page to download', 'also — download from hypeddit.com']
		});
		// The trailing link is a link all the same.
		const [, other] = openButtons('/bothartist/five');
		await click(other);
		expect(chrome.tabs.create.mock.calls.map((c) => c[0].url)).toEqual(['https://hypeddit.com/track/five']);
		expect(queued()).toEqual(['/bothartist/five']);
	});

	it.each([
		[
			'a SoundCloud download',
			[track('A', 'One', { details: details({ downloadable: true }) })],
			/^SoundCloud downloads live in each track’s More menu .* marks the track downloaded\.$/
		],
		[
			'a track without links',
			[track('A', 'One', { details: details() })],
			/^tracks with no link carry badges that search the artist on Beatport, Bandcamp, ZIPDJ, MyMP3Pool\.$/
		]
	])('explains only what the list needs: %s', async (_what, tracks, hint) => {
		panelChrome({ response: answer(tracks) });
		await load();
		expect(sections()[0].hint).toMatch(hint);
	});

	it('needs no hint for external links alone', async () => {
		panelChrome({
			response: answer([track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) })])
		});
		await load();
		expect(sections()[0].hint).toBe(null);
	});

	it('ignores an unknown saved sort and groups by site instead', async () => {
		localStorage.setItem('pref:sort', 'sideways');
		panelChrome({
			response: answer([track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) })])
		});
		await load();
		expect(sections().map((s) => s.heading)).toEqual(['hypeddit.com']);
	});
});

describe('filtering', () => {
	const ROWS = [
		track('A', 'One', { details: details({ purchaseUrl: 'https://hypeddit.com/one' }) }),
		track('B', 'Two', {
			details: details({ downloadable: true, purchaseUrl: 'https://hypeddit.com/two' })
		}),
		track('C', 'Three', { details: details() }),
		track('D', 'Four', { details: null })
	];

	const hideToggle = () => document.getElementById('hide-downloaded');
	const searchBox = () => document.querySelector<HTMLInputElement>('input[type="search"]');

	const countLine = () =>
		[...document.querySelectorAll('p')].find((p) => /shown|match|marked downloaded/.test(p.textContent))?.textContent ??
		'';

	const shownRows = () => sections().map((s) => [s.count, s.rows.map((r) => r.permalink)]);

	it('hides downloaded tracks on request, drops the sections that empties, and follows new marks', async () => {
		panelChrome({ response: answer(ROWS), storage: { downloaded: { '/a/one': ISO } } });
		await load();
		expect(countLine()).toBe('');
		expect(shownRows()).toEqual([
			['(1)', ['/b/two']],
			['(2)', ['/a/one', '/b/two']],
			['(1)', ['/c/three']],
			['(1)', ['/d/four']]
		]);

		await click(hideToggle());
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('1');
		expect(shownRows()).toEqual([
			['(1)', ['/b/two']],
			['(1 of 2)', ['/b/two']],
			['(1)', ['/c/three']],
			['(1)', ['/d/four']]
		]);
		expect(countLine()).toBe('3 of 4 shown');

		// A mark made while the toggle is on hides that track straight away.
		await click(markButton('/b/two', 1));
		expect(shownRows()).toEqual([
			['(1)', ['/c/three']],
			['(1)', ['/d/four']]
		]);
		expect(countLine()).toBe('2 of 4 shown');

		await click(markButton('/c/three'));
		await click(markButton('/d/four'));
		expect(shownRows()).toEqual([]);
		expect(countLine()).toBe('all 4 are marked downloaded.');

		await click(hideToggle());
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('0');
		expect(shownRows()).toEqual([
			['(1)', ['/b/two']],
			['(2)', ['/a/one', '/b/two']],
			['(1)', ['/c/three']],
			['(1)', ['/d/four']]
		]);
		expect(countLine()).toBe('');
	});

	it('narrows the list to the tracks matching the search — title, artist or site — in either order', async () => {
		localStorage.setItem('pref:hide-downloaded', '1');
		panelChrome({ response: answer(ROWS), storage: { downloaded: { '/a/one': ISO } } });
		await load();
		expect(countLine()).toBe('3 of 4 shown');

		await type(searchBox(), 'hype');
		expect(shownRows()).toEqual([['(1 of 2)', ['/b/two']]]);
		expect(countLine()).toBe('1 of 4 shown');

		await click(hideToggle());
		expect(shownRows()).toEqual([['(2)', ['/a/one', '/b/two']]]);
		expect(countLine()).toBe('2 of 4 shown');

		await type(searchBox(), '  C three ');
		expect(shownRows()).toEqual([['(1)', ['/c/three']]]);

		await type(searchBox(), 'soundcloud');
		expect(shownRows()).toEqual([['(1)', ['/b/two']]]);

		await type(searchBox(), 'nothing like it');
		expect(shownRows()).toEqual([]);
		expect(countLine()).toBe('nothing matches “nothing like it”.');

		await type(searchBox(), '');
		expect(countLine()).toBe('');
		expect(shownRows().map(([count]) => count)).toEqual(['(1)', '(2)', '(1)', '(1)']);
	});

	// The X inside the box, which is only there while there is something to
	// clear: the search field's own clear button is hidden (it draws over ours).
	it('clears the search from the button in the box', async () => {
		panelChrome({ response: answer(ROWS) });
		await load();
		expect(byLabel('clear the search')).toBeFalsy();

		await type(searchBox(), 'two');
		expect(shownRows()).toEqual([
			['(1)', ['/b/two']],
			['(1 of 2)', ['/b/two']]
		]);

		await click(byLabel('clear the search'));
		expect(searchBox()!.value).toBe('');
		expect(shownRows().map(([count]) => count)).toEqual(['(1)', '(2)', '(1)', '(1)']);
		expect(byLabel('clear the search')).toBeFalsy();
	});
});

describe('the cover art on a row', () => {
	// jsdom has none of what the copy runs on — no `createImageBitmap`, no
	// `OffscreenCanvas`, no `ClipboardItem` — so the decode-and-re-encode
	// pipeline is stood up here. `vi.stubGlobal` puts each back afterwards.
	function installImagePipeline() {
		const png = new Blob(['png bytes'], { type: 'image/png' });
		vi.stubGlobal(
			'fetch',
			vi.fn(async () => ({
				ok: true,
				status: 200,
				blob: async () => new Blob(['jpeg bytes'], { type: 'image/jpeg' })
			}))
		);
		vi.stubGlobal(
			'createImageBitmap',
			vi.fn(async () => ({ width: 1080, height: 1080 }))
		);
		vi.stubGlobal(
			'OffscreenCanvas',
			class {
				getContext() {
					return { drawImage() {} };
				}
				async convertToBlob() {
					return png;
				}
			}
		);
		vi.stubGlobal(
			'ClipboardItem',
			class {
				declare items: Record<string, Blob>;
				constructor(items: Record<string, Blob>) {
					this.items = items;
				}
			}
		);

		const clipboard = {
			writeText: vi.fn<(text: string) => Promise<void>>(async () => {}),
			write: vi.fn<(items: ClipboardItem[]) => Promise<void>>(async () => {})
		};

		Object.defineProperty(navigator, 'clipboard', { value: clipboard, configurable: true });

		return clipboard;
	}

	const thumb = () => document.querySelector('[aria-label="copy the cover art"]');

	it('ticks the thumbnail while it copies, then clears the tick', async () => {
		vi.useFakeTimers();
		const clipboard = installImagePipeline();
		panelChrome({
			response: answer([track('Bicep', 'Glue', { details: details({ downloadable: true }) })])
		});
		await load();

		await click(thumb());
		expect(text()).toMatch(/copied the cover at 1080 × 1080/);
		expect(clipboard.write).toHaveBeenCalledTimes(1);
		expect(globalThis.fetch).toHaveBeenCalledTimes(1);
		// The tick sits over the thumbnail for a moment, then goes.
		expect(thumb()!.querySelector('.text-good')).not.toBe(null);

		await vi.advanceTimersByTimeAsync(1500);
		await settle();
		expect(thumb()!.querySelector('.text-good')).toBe(null);
	});

	// One click, one fetch: clicking again while the first is still on its way
	// would copy the same image twice over.
	it('ignores a second click while the first is still fetching', async () => {
		vi.useFakeTimers();
		let finish!: () => void;
		const clipboard = installImagePipeline();
		vi.stubGlobal(
			'fetch',
			vi.fn(
				() =>
					new Promise((resolve) => {
						finish = () =>
							resolve({
								ok: true,
								status: 200,
								blob: async () => new Blob(['jpeg'], { type: 'image/jpeg' })
							});
					})
			)
		);
		panelChrome({
			response: answer([track('Bicep', 'Glue', { details: details({ downloadable: true }) })])
		});
		await load();

		await click(thumb());
		// The spinner is up, and the row is busy.
		expect(thumb()!.querySelector('.animate-spin')).not.toBe(null);
		await click(thumb());
		expect(globalThis.fetch).toHaveBeenCalledTimes(1);

		finish();
		await settle();
		expect(clipboard.write).toHaveBeenCalledTimes(1);
	});
});

describe('opening a row', () => {
	// The chevron on every row, named after the track it is on.
	const chevron = (permalink: string, nth = 0) => rowFor(permalink, nth).querySelector('[aria-expanded]');

	const isOpen = (permalink: string, nth = 0) => chevron(permalink, nth)!.getAttribute('aria-expanded') === 'true';

	// What the opened row says, as [label, value] — and whether each value can
	// be copied, which is what `dead` lists.
	const facts = (permalink: string, nth = 0) =>
		[...rowFor(permalink, nth).querySelectorAll('dl > div')].map((line) => [
			clean(line.querySelector('dt')),
			clean(line.querySelector('dd button'))
		]);

	// The two marks the opened row draws instead of spelling out, as the names
	// a screen reader reads — which is also the only place the state is said in
	// words, the rest of it being the way the icon is drawn.
	const marks = (permalink: string, nth = 0) =>
		[...rowFor(permalink, nth).querySelectorAll('[role="img"]')].map((el) => el.getAttribute('aria-label'));

	const markHints = (permalink: string, nth = 0) =>
		[...rowFor(permalink, nth).querySelectorAll('[role="img"]')].map((el) => el.getAttribute('title'));

	const factButton = (permalink: string, label: string, nth = 0) =>
		[...rowFor(permalink, nth).querySelectorAll('dl > div')]
			.find((line) => clean(line.querySelector('dt')) === label)
			?.querySelector('button');

	const installClipboard = (writeText = vi.fn<(text: string) => Promise<void>>(async () => {})) => {
		Object.defineProperty(navigator, 'clipboard', {
			value: { writeText },
			configurable: true
		});

		return writeText;
	};

	const LIKED = track('Bicep', 'Glue', {
		liked: true,
		reposted: false,
		details: details({ permalink: '/bicep/glue', downloadable: true, genre: 'Techno' })
	});

	it('shows the track as SoundCloud describes it, one field a line', async () => {
		panelChrome({ response: answer([LIKED]) });
		await load();

		expect(isOpen('/bicep/glue')).toBe(false);
		expect(rowFor('/bicep/glue').querySelector('dl')).toBe(null);

		await click(chevron('/bicep/glue'));
		expect(isOpen('/bicep/glue')).toBe(true);
		expect(facts('/bicep/glue')).toEqual([
			['title', 'Glue'],
			['artist', 'Bicep'],
			['genre', 'Techno'],
			['link', 'https://soundcloud.com/bicep/glue'],
			['from', 'My Playlist']
		]);
		// A yes and a no, as icons: neither is a line of its own down there.
		expect(marks('/bicep/glue')).toEqual(['liked', 'not reposted']);

		await click(chevron('/bicep/glue'));
		expect(isOpen('/bicep/glue')).toBe(false);
		expect(rowFor('/bicep/glue').querySelector('dl')).toBe(null);
	});

	// The genre the page itself rendered beside the row (Likes pages do), which
	// is the whole answer for a track nobody has looked up — and the one a
	// track that came in through the session queue has, since a queued track
	// carries what its page said and no lookup ever runs for it.
	it('shows the genre the page rendered, without waiting for a lookup', async () => {
		panelChrome({
			response: answer([track('Bicep', 'Glue', { genre: 'Drum & Bass', liked: true, reposted: false, details: null })])
		});
		await load();
		await click(chevron('/bicep/glue'));

		expect(facts('/bicep/glue')[2]).toEqual(['genre', 'Drum & Bass']);
		expect(marks('/bicep/glue')).toEqual(['liked', 'not reposted']);
	});

	// What a queued track carries is what its page said, and it is all there is
	// to say about those three: no lookup answers them, and the page a queued
	// track came off is not open any more.
	it('keeps what the page said about a track added from the queue', async () => {
		panelChrome({
			tab: { id: 1, url: 'https://hypeddit.com/gate' },
			session: {
				queue: [
					{
						permalink: '/a/one',
						title: 'One',
						artist: 'A',
						genre: 'Dubstep',
						liked: true,
						reposted: false
					}
				]
			}
		});
		await open();
		await click(addQueueButton());
		await click(chevron('/a/one'));

		expect(facts('/a/one')[2]).toEqual(['genre', 'Dubstep']);
		expect(marks('/a/one')).toEqual(['liked', 'not reposted']);
		expect(markHints('/a/one')).toEqual(['you’ve liked this on SoundCloud.', 'you haven’t reposted this.']);
	});

	// The three ways a field can have no answer, which are three different
	// things and only one of them is "no".
	it('says what it does not know rather than guessing at it', async () => {
		panelChrome({
			response: answer([
				// A badge tile: no like or repost button on the page to read.
				track('Tile', 'Unknown', {
					liked: null,
					reposted: null,
					details: details({ permalink: '/tile/unknown', downloadable: true })
				}),
				// Details cached before the genre was kept: no `genre` key at all.
				track('Old', 'Cache', {
					liked: false,
					reposted: true,
					details: (() => {
						const d = details({ permalink: '/old/cache', downloadable: true });
						delete d.genre;

						return d;
					})()
				}),
				// The uploader filed it under nothing, which SoundCloud did say.
				track('None', 'Set', {
					liked: false,
					reposted: false,
					details: details({ permalink: '/none/set', downloadable: true, genre: '' })
				})
			])
		});
		await load();

		for (const permalink of ['/tile/unknown', '/old/cache', '/none/set']) {
			await click(chevron(permalink));
		}

		expect(facts('/tile/unknown')).toEqual([
			['title', 'Unknown'],
			['artist', 'Tile'],
			['genre', 'unknown'],
			['link', 'https://soundcloud.com/tile/unknown'],
			['from', 'My Playlist']
		]);
		// Nothing on that page drew either button, and the marks say so rather
		// than drawing the same thing a "no" would.
		expect(marks('/tile/unknown')).toEqual(['liked: unknown', 'reposted: unknown']);
		expect(markHints('/tile/unknown')).toEqual([
			'the row this came off drew no like button.',
			'the row this came off drew no repost button.'
		]);
		expect(facts('/old/cache')[2]).toEqual(['genre', 'unknown']);
		expect(marks('/old/cache')).toEqual(['not liked', 'reposted']);
		expect(markHints('/old/cache')).toEqual(['you haven’t liked this.', 'you’ve reposted this on SoundCloud.']);
		// "" is an answer — the uploader set no genre — and reads as one.
		expect(facts('/none/set')[2]).toEqual(['genre', 'none']);
	});

	// The way off the list from the row itself: four buttons is as many as a
	// 400px row will take, so this lives where there is room to say what it does.
	it('takes the track off the list from the row it opens', async () => {
		panelChrome({ response: answer([LIKED, track('Other', 'Track', { details: details() })]) });
		await load();
		await click(chevron('/bicep/glue'));

		await click(byLabel('take Glue off the list'));
		expect(listedTracks()).toEqual(['/other/track']);
		expect(rowFor('/bicep/glue')).toBe(undefined);
		expect(text()).toMatch(/took Glue off the list/);
	});

	// An entry restored from a file written by a version that did not record
	// where a track came from: the field says so rather than naming a page
	// nobody named.
	it('says “unknown” for a track nothing says where it came from', async () => {
		panelChrome({
			response: answer([]),
			storage: {
				listed: [
					{
						permalink: '/bicep/glue',
						title: 'Glue',
						artist: 'Bicep',
						details: details({ permalink: '/bicep/glue', downloadable: true })
					}
				]
			}
		});
		await open();
		await click(chevron('/bicep/glue'));
		expect(facts('/bicep/glue').at(-1)).toEqual(['from', 'unknown']);
		expect(factButton('/bicep/glue', 'from')!.disabled).toBe(true);
	});

	// The settings switch ($lib/title-case.svelte.js). The transform is tested
	// on its own in test/lib/title-case.test.ts; what is here is which fields it
	// reaches, and that a click copies what the row is showing.
	describe('with title case switched on', () => {
		const SHOUTED = track('BICEP', 'GLUE (ORIGINAL MIX)', {
			genre: 'drum & bass',
			details: details({ permalink: '/bicep/glueoriginalmix', downloadable: true })
		});

		it('cases the three fields that are words, and nothing else', async () => {
			localStorage.setItem('pref:title-case', '1');
			panelChrome({ response: answer([SHOUTED]) });
			await load();
			await click(chevron('/bicep/glueoriginalmix'));

			expect(facts('/bicep/glueoriginalmix')).toEqual([
				['title', 'Glue (Original Mix)'],
				['artist', 'Bicep'],
				['genre', 'Drum & Bass'],
				// An address is not words, and case is meaning in one; "from" is this
				// list's own sentence about where the track came in.
				['link', 'https://soundcloud.com/bicep/glueoriginalmix'],
				['from', 'My Playlist']
			]);
		});

		it('leaves a field nobody answered saying what it said', async () => {
			localStorage.setItem('pref:title-case', '1');
			panelChrome({
				response: answer([
					track('Tile', 'Unknown', {
						liked: null,
						details: details({ permalink: '/tile/unknown', downloadable: true })
					})
				])
			});
			await load();
			await click(chevron('/tile/unknown'));
			expect(facts('/tile/unknown')[2]).toEqual(['genre', 'unknown']);
			expect(factButton('/tile/unknown', 'genre')!.disabled).toBe(true);
		});

		it('copies what the row is showing', async () => {
			const writeText = installClipboard();
			localStorage.setItem('pref:title-case', '1');
			panelChrome({ response: answer([SHOUTED]) });
			await load();
			await click(chevron('/bicep/glueoriginalmix'));

			await click(factButton('/bicep/glueoriginalmix', 'title'));
			expect(writeText).toHaveBeenCalledWith('Glue (Original Mix)');
		});

		it('is off until it is set, and leaves the words as SoundCloud gave them', async () => {
			panelChrome({ response: answer([SHOUTED]) });
			await load();
			await click(chevron('/bicep/glueoriginalmix'));
			expect(facts('/bicep/glueoriginalmix').slice(0, 3)).toEqual([
				['title', 'GLUE (ORIGINAL MIX)'],
				['artist', 'BICEP'],
				['genre', 'drum & bass']
			]);
		});
	});

	it('copies a field, and refuses to copy a “we don’t know”', async () => {
		const writeText = installClipboard();
		panelChrome({
			response: answer([
				LIKED,
				track('Tile', 'Unknown', {
					liked: null,
					details: details({ permalink: '/tile/unknown', downloadable: true })
				})
			])
		});
		await load();
		await click(chevron('/bicep/glue'));
		await click(chevron('/tile/unknown'));

		await click(factButton('/bicep/glue', 'title'));
		expect(writeText).toHaveBeenCalledWith('Glue');
		expect(text()).toMatch(/copied the title/);

		await click(factButton('/bicep/glue', 'link'));
		expect(writeText).toHaveBeenLastCalledWith('https://soundcloud.com/bicep/glue');
		expect(writeText).toHaveBeenCalledTimes(2);

		// Nothing was said about this one, so there is nothing to put on a
		// clipboard: the word "unknown" is the panel talking about itself.
		const unknown = factButton('/tile/unknown', 'genre')!;
		expect(unknown.disabled).toBe(true);
		expect(unknown.getAttribute('aria-label')).toBe('genre: unknown');
		expect(unknown.title).toBe('');
	});

	it('says so when the clipboard refuses', async () => {
		installClipboard(vi.fn(async () => Promise.reject(new Error('no focus'))));
		// The fallback route is the old selection one, which jsdom has no
		// execCommand for either.
		// @ts-expect-error -- a browser with no execCommand at all, which the DOM types never allow
		document.execCommand = undefined;
		panelChrome({ response: answer([LIKED]) });
		await load();
		await click(chevron('/bicep/glue'));
		await click(factButton('/bicep/glue', 'artist'));
		expect(text()).toMatch(/couldn’t copy the artist/);
	});

	it('opens the track in a tab from the arrow beside its address', async () => {
		panelChrome({ response: answer([LIKED]) });
		await load();
		await click(chevron('/bicep/glue'));
		await click(byLabel('open the track in a new tab'));
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: 'https://soundcloud.com/bicep/glue',
			active: false
		});
		// Opening a download link queues the track; looking at it does not.
		expect(queued()).toEqual([]);
	});

	// A row is open by track, not by position: the by-site layout lists a track
	// offering both kinds of link in two sections, and it is one track.
	it('opens a track wherever the page lists it, and stays open when the popup closes', async () => {
		panelChrome({
			response: answer([
				track('Both', 'Five', {
					liked: true,
					reposted: true,
					details: details({
						permalink: '/both/five',
						downloadable: true,
						purchaseUrl: 'https://hypeddit.com/five'
					})
				})
			])
		});
		await load();
		expect(rowFor('/both/five', 1)).not.toBe(undefined);

		await click(chevron('/both/five'));
		expect(isOpen('/both/five', 1)).toBe(true);
		expect(marks('/both/five', 1)).toEqual(['liked', 'reposted']);

		await reopenPanel('manager');
		expect(isOpen('/both/five')).toBe(true);
		expect(facts('/both/five')[0]).toEqual(['title', 'Five']);
	});

	// Title and artist are on screen before a single lookup has come back, so
	// the chevron has something to open from the first frame.
	it('opens a row that is still being checked', async () => {
		panelChrome();
		chrome.tabs.sendMessage.mockImplementation(async (_id, message) =>
			message.type === 'EXTRACT_TRACKS' ? answer([track('Waiting', 'Soon')]) : new Promise(() => {})
		);
		await load();
		await click(chevron('/waiting/soon'));
		expect(facts('/waiting/soon')).toEqual([
			['title', 'Soon'],
			['artist', 'Waiting'],
			['genre', 'unknown'],
			['link', 'https://soundcloud.com/waiting/soon'],
			['from', 'My Playlist']
		]);
		expect(marks('/waiting/soon')).toEqual(['liked: unknown', 'reposted: unknown']);
	});
});

// The backup itself is the settings sheet's (test/saved-state.test.ts): the
// file carries the marks a SoundCloud page paints, the content script's cache
// of lookups, the session queue and three panels' options, none of which is
// this tool's. What stays here is the way to it — the list is the biggest
// thing in the file, so the footer is where somebody looks.
describe('the footer', () => {
	it('points at settings rather than holding the backup itself', async () => {
		panelChrome({ response: undefined });
		await open();
		const go = button('back up or restore…');
		expect(go).toBeTruthy();
		expect(go!.getAttribute('title')).toMatch(/^in settings:/);
		// No panel draws a sheet — the shell does, over whichever tool is open —
		// so what this press can be seen to do here is set the place.
		await click(go);
		await settle();
		expect(chrome.testStore('session').view.settings).toBe(true);
	});

	// Restoring a backup is something one does *before* opening a playlist, so
	// the way to it is there on a tab with nothing on it.
	it('is there whatever the active tab is', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		await open();
		expect(rows()).toHaveLength(0);
		expect(button('back up or restore…')).toBeTruthy();
	});
});
