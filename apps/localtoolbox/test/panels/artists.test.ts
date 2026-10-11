// src/tools/artists/Panel.svelte — the artists tool: a kept list of the
// uploaders behind the tracks somebody has added, and their socials.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
	buttons,
	button,
	byLabel,
	chooseOption,
	click,
	extraction,
	installClipboard,
	labelled,
	mountPanel,
	openSelect,
	panelChrome,
	reopenPanel,
	rows,
	rowText,
	settle,
	SOUNDCLOUD_TAB,
	text,
	track,
	type
} from '../helpers/panel.ts';
import type { ChromeMock, PageReply, Seed, TabStub } from '../helpers/chrome.ts';
import type { ArtistEntry, ArtistProfile, LookupError, PageFailure, ProfilesAnswer } from '$lib/types';

declare const chrome: ChromeMock;

/** What `chromeAnswering` is told: the page's answer, the lookups', and the chrome around them. */
interface Answering {
	page?: ReturnType<typeof extraction>;
	/** An Error makes the lookup's message fail. */
	lookup?: ProfilesAnswer | PageFailure | Error;
	tab?: TabStub;
	storage?: Seed;
	session?: Seed;
}

const TRACKS = [track('Bicep', 'Glue'), track('Bicep', 'Aura'), track('Four Tet', 'Baby')];

const PAGE = extraction({ tracks: TRACKS });

const profile = (permalink: string, extra: Partial<ArtistProfile> = {}) => ({
	id: 7,
	permalink,
	username: 'Bicep',
	fullName: '',
	city: 'Belfast',
	country: 'GB',
	followers: 1234,
	trackCount: 1,
	verified: true,
	avatarUrl: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
	bio: '',
	links: [{ network: 'instagram', title: '', url: 'https://www.instagram.com/bicepmusic/' }],
	...extra
});

const answer = (profiles: ArtistProfile[] = [], failed: string[] = [], error: LookupError = ''): ProfilesAnswer => ({
	ok: true,
	profiles,
	failed,
	error
});

/** An artist as the list keeps one. */
const entry = (permalink: string, extra: Partial<ArtistEntry> = {}) => ({
	permalink,
	name: permalink.slice(1),
	tracks: [{ permalink: `${permalink}/one`, title: 'One' }],
	profile: null,
	from: 'My Playlist',
	at: 1,
	...extra
});

/** A chrome whose page answers `page` and whose lookups answer `lookup`. */
function chromeAnswering({ page = PAGE, lookup = answer(), tab = SOUNDCLOUD_TAB, ...rest }: Answering = {}) {
	const mock = panelChrome({ tab, ...rest });
	mock.tabs.sendMessage.mockImplementation(async (_id, message) => {
		if (message.type === 'EXTRACT_TRACKS') return page;

		if (lookup instanceof Error) throw lookup;

		return lookup;
	});

	return mock;
}

const load = () => mountPanel('artists');

const stored = () => chrome.testStore('local').artists ?? [];

const addButton = () => buttons().find((b) => (b.getAttribute('aria-label') ?? '').endsWith('from this tab'));

const addTab = () => click(addButton());

const lookups = () =>
	chrome.tabs.sendMessage.mock.calls
		.map(([, message]) => message)
		.filter((message) => message.type === 'GET_ARTIST_PROFILES');

const meta = () => document.querySelector('header p')?.textContent.trim() ?? '';

const status = () => document.querySelector('[role="status"], [role="alert"]');

const badges = () => [...document.querySelectorAll('li [aria-label^="open "]')];

const chevron = (name: string) => byLabel(`about ${name}`);

describe('before anything is added', () => {
	it('opens on an empty list, offering the tab in front', async () => {
		chromeAnswering();
		await load();
		expect(text()).toMatch(/nobody here yet/);
		expect(addButton()!.getAttribute('aria-label')).toBe('add 2 artists from this tab');
		expect(addButton()!.disabled).toBe(false);
		expect(text()).toMatch(/by Curator · 2 artists · 3 tracks/);
		expect(meta()).toBe('');
		expect(rows()).toEqual([]);
	});

	it('says what the tab is when it is not a page to add from, and still opens', async () => {
		chromeAnswering({ tab: { id: 1, url: 'https://example.com/' } });
		await load();
		expect(text()).toMatch(/not a SoundCloud playlist, likes or track page/);
		expect(addButton()!.disabled).toBe(true);
		expect(addButton()!.getAttribute('aria-label')).toBe('add from this tab');
	});

	it('explains a page that could not be read', async () => {
		panelChrome({ response: new Error('Could not establish connection') });
		await load();
		expect(text()).toMatch(/can’t reach the page/);
	});
});

describe('adding from the tab', () => {
	it('lists the uploaders, looks them up, and draws what came back', async () => {
		chromeAnswering({ lookup: answer([profile('/bicep')], ['/fourtet']) });
		await load();
		await addTab();

		expect(text()).toMatch(/added 2 artists from My Playlist/);
		expect(lookups()).toEqual([{ type: 'GET_ARTIST_PROFILES', permalinks: ['/bicep', '/fourtet'] }]);
		expect(stored().map((one) => [one.permalink, one.name, one.from])).toEqual([
			['/bicep', 'Bicep', 'My Playlist'],
			['/fourtet', 'Four Tet', 'My Playlist']
		]);
		expect(stored()[0].tracks).toEqual([
			{ permalink: '/bicep/glue', title: 'Glue' },
			{ permalink: '/bicep/aura', title: 'Aura' }
		]);
		expect(stored()[0].profile).toMatchObject({ permalink: '/bicep', city: 'Belfast' });
		expect(stored()[1].profile).toMatchObject({ unavailable: true });

		expect(rowText()).toEqual(['Bicep Belfast, GB · 1.2K followers · 1 link IG', 'Four Tet no public profile']);
		expect(meta()).toBe('2 artists · 1 with socials');
		expect(badges().map((b) => b.getAttribute('aria-label'))).toEqual([
			'open Instagram: https://www.instagram.com/bicepmusic/'
		]);
		expect(document.querySelector('li img')!.getAttribute('src')).toBe(profile('/bicep').avatarUrl);
		expect(document.querySelector('li [aria-label="verified"]')).toBeTruthy();
		// No SoundCloud tab is needed to see it again.
		expect(addButton()!.getAttribute('aria-label')).toBe('refresh 2 artists from this tab');
	});

	it('fills the rows in as the answers arrive, and shows how far it is', async () => {
		let finish!: (value: PageReply) => void;
		chromeAnswering();
		chrome.tabs.sendMessage.mockImplementation(
			(_id, message) =>
				new Promise((resolve) => {
					if (message.type === 'EXTRACT_TRACKS') resolve(PAGE);
					else finish = resolve;
				})
		);
		await load();
		await addTab();

		expect(rowText()).toEqual(['Bicep looking up…', 'Four Tet looking up…']);
		expect(document.querySelectorAll('li [aria-label="looking up"]')).toHaveLength(2);
		expect(text()).toMatch(/looking up 2 artists…/);

		await chrome.testSendMessage({
			type: 'ARTIST_PROFILES_PROGRESS',
			done: 1,
			total: 2,
			profiles: [profile('/bicep')],
			failed: []
		});
		// The Manager's lookup reports progress on the same channel; mid-lookup is
		// exactly when one of its answers must not be taken for this one's.
		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 1,
			total: 1,
			profiles: [profile('/fourtet')]
		});
		await settle();
		expect(text()).toMatch(/looking up socials… 1 \/ 2/);
		// Its own answer is in, so the row shows it — badge and all — while the
		// other is still being asked about.
		expect(rowText()).toEqual(['Bicep Belfast, GB · 1.2K followers · 1 link IG', 'Four Tet looking up…']);
		expect(document.querySelectorAll('li [aria-label="looking up"]')).toHaveLength(1);

		finish(answer([profile('/bicep')], ['/fourtet']));
		await settle();
		expect(text()).not.toMatch(/looking up socials/);
		expect(rowText()).toEqual(['Bicep Belfast, GB · 1.2K followers · 1 link IG', 'Four Tet no public profile']);
		expect(stored()[1].profile).toMatchObject({ unavailable: true });
	});

	it('ignores progress that is not for it', async () => {
		// An artist the stray answers name, so taking any of them would show.
		chromeAnswering({ storage: { artists: [entry('/bicep')] } });
		await load();
		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 1,
			total: 1,
			profiles: [profile('/bicep')]
		});
		await chrome.testSendMessage(null);
		// Its own kind of answer, but to a lookup this panel never started.
		await chrome.testSendMessage({ type: 'ARTIST_PROFILES_PROGRESS', profiles: [profile('/bicep')] });
		await settle();
		expect(rowText()).toEqual(['bicep not looked up']);
	});

	it('says so when the lookup could not be made, and keeps the artists', async () => {
		chromeAnswering({ lookup: new Error('Could not establish connection') });
		await load();
		await addTab();
		expect(status()!.textContent).toMatch(/couldn’t look up their socials\. can’t reach the page/);
		expect(rowText()).toEqual(['Bicep not looked up', 'Four Tet not looked up']);
		expect(text()).toMatch(/2 artists not looked up\./);
		expect(meta()).toBe('2 artists · 0 with socials · 2 not looked up');
	});

	it('says so when SoundCloud rejected the lookups, keeping what came back', async () => {
		chromeAnswering({ lookup: answer([profile('/bicep')], [], 'unauthorized') });
		await load();
		await addTab();
		expect(status()!.textContent).toMatch(/some socials weren’t looked up\. SoundCloud rejected/);
		expect(rowText()[0]).toMatch(/^Bicep Belfast/);
		expect(rowText()[1]).toBe('Four Tet not looked up');
	});

	it('says so when the answer cannot be read at all', async () => {
		chromeAnswering({ lookup: { ok: false, error: 'boom' } });
		await load();
		await addTab();
		expect(status()!.textContent).toMatch(/couldn’t look them up\. reload the SoundCloud tab/);
	});

	it('refreshes rather than duplicates, and looks up only what has no answer', async () => {
		chromeAnswering({
			lookup: answer([profile('/bicep')], ['/fourtet']),
			storage: { artists: [entry('/bicep', { profile: { ...profile('/bicep'), at: 1 } })] }
		});
		await load();
		expect(addButton()!.getAttribute('aria-label')).toBe('add 1 artist from this tab');
		await addTab();
		expect(text()).toMatch(/added 1 artist, refreshed 1 artist from My Playlist/);
		expect(lookups()).toEqual([{ type: 'GET_ARTIST_PROFILES', permalinks: ['/fourtet'] }]);
		expect(stored()).toHaveLength(2);
	});

	it('says when the list is full, and adds nothing', async () => {
		chromeAnswering({
			storage: { artists: Array.from({ length: 2000 }, (_, n) => entry(`/a${n}`)) },
			// A search that hides every row: two thousand of them are not what this
			// test is about, and the list holds them either way.
			session: { view: { open: 'artists', panels: { artists: { query: 'zzzz' } } } }
		});
		await load();
		expect(rows()).toEqual([]);
		expect(text()).toMatch(/nothing matches “zzzz”/);
		await addTab();
		expect(status()!.textContent).toMatch(
			/nothing to add from My Playlist\. 2 artists wouldn’t fit: the list holds 2000\./
		);
		expect(lookups()).toEqual([]);
	});

	it('adds without asking anyone when the tab is not SoundCloud', async () => {
		chromeAnswering({ tab: { id: 1, url: 'https://example.com/' }, storage: { listed: TRACKS } });
		await load();
		await click(button('Manager (2)'));
		expect(stored()).toHaveLength(2);
		expect(lookups()).toEqual([]);
		expect(text()).toMatch(/2 artists not looked up — open a SoundCloud tab to look up socials\./);
		// Not asked and refused: not asked at all, so there is no failure to report.
		expect(text()).not.toMatch(/couldn’t look up their socials/);
		expect(labelled('look up 2 artists', HTMLButtonElement).disabled).toBe(true);
	});
});

describe('the other sources', () => {
	it('adds the artists behind the Manager’s list, and says where from', async () => {
		chromeAnswering({ storage: { listed: TRACKS } });
		await load();
		const manager = button('Manager (2)')!;
		expect(manager.getAttribute('aria-label')).toBe('add the Manager’s list — 2 artists');
		await click(manager);
		expect(text()).toMatch(/added 2 artists from the Manager’s list/);
		expect(stored().map((one) => one.from)).toEqual(['the Manager’s list', 'the Manager’s list']);
		// All here now: the button counts the whole source and is off.
		expect(button('Manager (2)')!.disabled).toBe(true);
		expect(button('Manager (2)')!.title).toMatch(/all 2 artists behind the Manager’s tracks are here/);
	});

	it('adds the artists behind the session queue', async () => {
		chromeAnswering({ session: { queue: [TRACKS[2]] } });
		await load();
		const queued = button('queue (1)')!;
		expect(queued.getAttribute('aria-label')).toBe('add the session queue — 1 artist');
		await click(queued);
		expect(text()).toMatch(/added 1 artist from the session queue/);
		expect(stored()[0]).toMatchObject({ permalink: '/fourtet', from: 'the session queue' });
		expect(button('queue (1)')!.disabled).toBe(true);
		expect(button('queue (1)')!.title).toMatch(/all 1 artist behind the queued tracks are here/);
	});

	it('offers neither when neither holds anything', async () => {
		chromeAnswering();
		await load();
		expect(button('Manager (')).toBeUndefined();
		expect(button('queue (')).toBeUndefined();
	});
});

describe('looking up from the bar', () => {
	it('asks about everyone not looked up, from any SoundCloud page', async () => {
		chromeAnswering({
			tab: { id: 7, url: 'https://soundcloud.com/stream' },
			page: extraction({ pageType: 'other', tracks: [] }),
			lookup: answer([profile('/a')], ['/b']),
			storage: { artists: [entry('/a'), entry('/b'), entry('/c', { profile: { unavailable: true, at: 1 } })] }
		});
		await load();
		expect(text()).toMatch(/that’s not a playlist, likes or track page/);
		const lookUp = labelled('look up 2 artists', HTMLButtonElement);
		expect(lookUp.disabled).toBe(false);
		expect(lookUp.title).toMatch(/asks SoundCloud who’s behind each path/);
		await click(lookUp);
		expect(lookups()).toEqual([{ type: 'GET_ARTIST_PROFILES', permalinks: ['/a', '/b'] }]);
		expect(rowText()).toEqual([
			'a Belfast, GB · 1.2K followers · 1 link IG',
			'b no public profile',
			'c no public profile'
		]);
		expect(byLabel('look up 2 artists')).toBeUndefined();
	});
});

describe('the list', () => {
	const THREE = [
		entry('/zed', { name: 'Zed', profile: { ...profile('/zed', { followers: 5, links: [] }), at: 1 } }),
		entry('/alpha', { name: 'alpha', profile: { unavailable: true, at: 1 } }),
		entry('/mid', {
			name: 'Mid',
			profile: {
				...profile('/mid', {
					followers: 99,
					city: '',
					country: '',
					avatarUrl: '',
					bio: 'find me at https://x.com/mid and mid@example.com',
					links: [
						{ network: 'instagram', title: '', url: 'https://instagram.com/mid' },
						{ network: 'facebook', title: '', url: 'https://facebook.com/mid' },
						{ network: 'youtube', title: '', url: 'https://youtube.com/@mid' },
						{ network: 'spotify', title: '', url: 'https://open.spotify.com/artist/mid' },
						{ network: 'bandcamp', title: '', url: 'https://mid.bandcamp.com' }
					]
				}),
				at: 1
			}
		})
	];

	beforeEach(() => {
		chromeAnswering({ storage: { artists: THREE } });
	});

	it('is sorted by name to begin with, and by list order or followers on request', async () => {
		await load();
		expect(rowText().map((line) => line.split(' ')[0])).toEqual(['alpha', 'Mid', 'Zed']);
		const offered = (await openSelect('sort order')).map((option) => option.textContent.trim());
		expect(offered).toEqual(['by name', 'list order', 'most followed']);

		await chooseOption('sort order', 'list order');
		expect(rowText().map((line) => line.split(' ')[0])).toEqual(['Zed', 'alpha', 'Mid']);
		expect(localStorage.getItem('pref:artists-sort')).toBe('list');

		await chooseOption('sort order', 'most followed');
		expect(rowText().map((line) => line.split(' ')[0])).toEqual(['Mid', 'Zed', 'alpha']);
	});

	it('shows a handful of badges and counts the rest, and draws a placeholder for no picture', async () => {
		await load();
		const mid = rows()[1];
		expect(mid.textContent).toMatch(/99 followers · 7 links/);
		expect(mid.querySelectorAll('[aria-label^="open "]')).toHaveLength(3);
		expect(mid.textContent).toMatch(/\+4/);
		expect(mid.querySelector('[title="no picture"]')).toBeTruthy();
		expect(mid.querySelector('img')).toBe(null);
	});

	it('opens a link in a tab behind the popup, or in front when asked', async () => {
		await load();
		await click(byLabel('open Instagram: https://instagram.com/mid'));
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: 'https://instagram.com/mid', active: false });

		await click(document.getElementById('background'));
		expect(localStorage.getItem('pref:background')).toBe('0');
		await click(byLabel('open Instagram: https://instagram.com/mid'));
		expect(chrome.tabs.create).toHaveBeenLastCalledWith({ url: 'https://instagram.com/mid', active: true });
	});

	it('is searched by everything known, and says when nothing matches', async () => {
		await load();
		const search = labelled('search the artists', HTMLInputElement);
		await type(search, 'example.com');
		expect(rowText()).toHaveLength(1);
		expect(rowText()[0]).toMatch(/^Mid/);
		expect(text()).toMatch(/1 of 3 shown/);

		await type(search, 'nobody at all');
		expect(rows()).toEqual([]);
		expect(text()).toMatch(/nothing matches “nobody at all”/);

		await click(byLabel('clear the search'));
		expect(rows()).toHaveLength(3);
		expect(search.value).toBe('');
	});

	it('copies every artist shown as text', async () => {
		const clipboard = installClipboard();
		await load();
		await type(byLabel('search the artists'), 'zed');
		await click(byLabel('copy 1 artist as text'));
		expect(clipboard.writeText).toHaveBeenCalledWith('Zed\nhttps://soundcloud.com/zed');
		expect(text()).toMatch(/copied 1 artist/);
	});

	it('says when the copy failed', async () => {
		const clipboard = installClipboard();
		clipboard.writeText.mockRejectedValue(new Error('not focused'));
		document.execCommand = vi.fn<typeof document.execCommand>(() => false);
		await load();
		await click(byLabel('copy 3 artists as text'));
		expect(text()).toMatch(/couldn’t copy the list/);
	});
});

describe('a row opened out', () => {
	const MID = entry('/mid', {
		name: 'Mid',
		from: '',
		profile: {
			...profile('/mid', {
				username: 'MID.',
				bio: 'hello',
				links: [{ network: 'instagram', title: 'my IG', url: 'https://instagram.com/mid' }]
			}),
			at: 1
		}
	});

	beforeEach(() => {
		chromeAnswering({ storage: { artists: [MID, entry('/other')] } });
	});

	it('lists every field, copies one on a click, and opens an address', async () => {
		const clipboard = installClipboard();
		await load();
		expect(document.querySelector('li dl')).toBe(null);
		await click(chevron('Mid'));
		expect(chevron('Mid')!.getAttribute('aria-expanded')).toBe('true');

		const dl = document.querySelector('li dl')!;
		expect(dl.getAttribute('aria-label')).toBe('about Mid');
		const labels = [...dl.querySelectorAll('dt')].map((dt) => dt.textContent.trim());
		expect(labels).toEqual(['SoundCloud', 'Instagram', 'name', 'place', 'followers', 'bio', 'tracks', 'from']);

		await click(byLabel('copy the Instagram'));
		expect(clipboard.writeText).toHaveBeenCalledWith('https://instagram.com/mid');
		expect(text()).toMatch(/copied the Instagram/);
		expect(labelled('copy the Instagram', HTMLElement).title).toMatch(/the arrow opens it/);
		expect(labelled('copy the bio', HTMLElement).title).toBe('click to copy');

		await click(byLabel('open Instagram in a new tab'));
		expect(chrome.tabs.create).toHaveBeenLastCalledWith({ url: 'https://instagram.com/mid', active: false });

		// A field nobody answered cannot be copied, and says so instead.
		const from = labelled('from: unknown', HTMLButtonElement);
		expect(from.disabled).toBe(true);

		await click(chevron('Mid'));
		expect(document.querySelector('li dl')).toBe(null);
	});

	it('searches the shops for the artist, with the badges switched on in settings', async () => {
		await load();
		await click(chevron('Mid'));
		const beatport = labelled('search Beatport for Mid', HTMLElement);
		expect(beatport.title).toBe('search Beatport for “Mid”');
		await click(beatport);
		expect(chrome.tabs.create).toHaveBeenLastCalledWith({
			url: 'https://www.beatport.com/search?q=Mid',
			active: false
		});
		expect(byLabel('search Bandcamp for Mid')).toBeTruthy();
	});

	it('asks SoundCloud about the one artist again', async () => {
		chromeAnswering({
			storage: { artists: [MID] },
			lookup: answer([profile('/mid', { city: 'Derry' })])
		});
		await load();
		await click(chevron('Mid'));
		const again = labelled('look up Mid again', HTMLButtonElement);
		expect(again.title).toMatch(/asks SoundCloud about this artist again/);
		await click(again);
		expect(lookups()).toEqual([{ type: 'GET_ARTIST_PROFILES', permalinks: ['/mid'] }]);
		expect(rowText()[0]).toMatch(/Derry, GB/);
	});

	it('is off to look up again without a SoundCloud tab', async () => {
		chromeAnswering({ storage: { artists: [MID] }, tab: { id: 1, url: 'https://example.com/' } });
		await load();
		await click(chevron('Mid'));
		const again = labelled('look up Mid again', HTMLButtonElement);
		expect(again.disabled).toBe(true);
		expect(again.title).toMatch(/open a SoundCloud tab/);
	});

	it('takes the artist off the list', async () => {
		await load();
		await click(chevron('Mid'));
		await click(byLabel('take Mid off the list'));
		expect(text()).toMatch(/took Mid off the list/);
		expect(rowText()).toEqual(['other not looked up']);
		expect(stored().map((one) => one.permalink)).toEqual(['/other']);
	});

	// A state file can restore a profile as little as `{ at }`: a follower count
	// it does not carry is not known, and the row says and opens as for null.
	it('opens a restored profile that carries no follower count', async () => {
		chromeAnswering({ storage: { artists: [entry('/bare', { name: 'Bare', profile: { at: 1 } })] } });
		await load();
		expect(rowText()).toEqual(['Bare no links']);
		await click(chevron('Bare'));
		expect(labelled('followers: unknown', HTMLButtonElement).disabled).toBe(true);
	});

	it('stays open when the popup comes back', async () => {
		await load();
		await click(chevron('Mid'));
		await reopenPanel('artists');
		expect(chevron('Mid')!.getAttribute('aria-expanded')).toBe('true');
		expect(document.querySelector('li dl')).toBeTruthy();
	});
});

describe('when the browser says no', () => {
	it('still draws with a runtime that cannot be listened to', async () => {
		chromeAnswering();
		chrome.runtime.onMessage.addListener.mockImplementation(() => {
			throw new Error('no runtime');
		});
		await load();
		expect(text()).toMatch(/nobody here yet/);
	});
});
