// src/tools/artists/Panel.svelte, the artists tool: a kept list of the
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
	stepFrames,
	SOUNDCLOUD_TAB,
	text,
	track,
	type
} from '../helpers/panel.ts';
import type { ChromeMock, PageReply, Seed, TabStub } from '../helpers/chrome.ts';
import type { ArtistEntry, ArtistProfile, ProfilesAnswer } from '$lib/types';

declare const chrome: ChromeMock;

const LOAD_HINT = 'open their SoundCloud page, then come back here.';

/** What `chromeAnswering` is told: the page's answer, the profiles', and the chrome around them. */
interface Answering {
	page?: ReturnType<typeof extraction>;
	/** What the tab answers GET_ARTIST_PROFILES with. An Error makes the message fail. */
	lookup?: PageReply | Error;
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

/** The tab's answer: the profiles it has loaded, of those asked about. */
const answer = (profiles: ArtistProfile[] = []): ProfilesAnswer => ({ ok: true, profiles });

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

/**
 * An artists list as the store leaves one: the entries in the local area with no profile, and the
 * profiles in the session area by path, beside whatever else `session` holds.
 */
const held = (artists: ArtistEntry[], session: Seed = {}) => ({
	storage: { artists: artists.map((one) => ({ ...one, profile: null })) },
	session: {
		...session,
		artistProfiles: Object.fromEntries(
			artists.flatMap((one) => (one.profile ? [[one.permalink, one.profile] as const] : []))
		)
	}
});

/** A chrome whose page answers `page` and whose profile reads answer `lookup`. */
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

/** The profiles as the session area holds them, by artist path. */
const sessionProfiles = async () => (await chrome.storage.session.get('artistProfiles')).artistProfiles;

const addButton = () => buttons().find((b) => (b.getAttribute('aria-label') ?? '').endsWith('from this tab'));

const addTab = () => click(addButton());

const lookups = () =>
	chrome.tabs.sendMessage.mock.calls
		.map(([, message]) => message)
		.filter((message) => message.type === 'GET_ARTIST_PROFILES');

const meta = () => document.querySelector('header p')?.textContent.trim() ?? '';

const status = () => document.querySelector('[role="status"], [role="alert"]');

/** The line saying how many artists have no socials yet, and how to get them. */
const incompleteLine = () => document.querySelector('p[role="status"]')?.textContent.trim() ?? '';

/** What the line under an artist's name says when hovered. */
const lineTitle = (permalink: string) =>
	rows()
		.find((li) => li.dataset.permalink === permalink)!
		.querySelectorAll('p')[1]
		.getAttribute('title');

// A link badge is named by its letters and what it does ("IG: open Instagram"), never the address.
const badges = () => [...document.querySelectorAll('li [aria-label*=": open "]')];

// An opened row's field, named by the value it copies.
const field = (value: string) =>
	[...document.querySelectorAll<HTMLButtonElement>('li dd button')].find((b) => b.textContent.trim() === value);

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
		// Nobody on the list, so nothing to fill in.
		expect(lookups()).toEqual([]);
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
	it('lists the uploaders, fills them in from the tab, and draws what came back', async () => {
		chromeAnswering({ lookup: answer([profile('/bicep')]) });
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
		// The profile is somebody else's account: kept for the session, and
		// never in the list that lasts.
		expect(stored().map((one) => one.profile)).toEqual([null, null]);
		expect(await sessionProfiles()).toMatchObject({ '/bicep': { permalink: '/bicep', city: 'Belfast' } });

		// The tab had not loaded Four Tet, so that row says so, and how to.
		expect(rowText()).toEqual(['Bicep Belfast, GB · 1.2K followers · 1 link IG', 'Four Tet not loaded yet']);
		expect(lineTitle('/fourtet')).toBe(`not loaded yet. ${LOAD_HINT}`);
		expect(lineTitle('/bicep')).toBe('Belfast, GB · 1.2K followers · 1 link');
		expect(incompleteLine()).toBe(`1 artist without socials yet. ${LOAD_HINT}`);
		expect(meta()).toBe('2 artists · 1 with socials · 1 not loaded yet');
		expect(badges().map((b) => [b.getAttribute('aria-label'), b.getAttribute('title')])).toEqual([
			['IG: open Instagram', 'https://www.instagram.com/bicepmusic/']
		]);
		expect(document.querySelector('li img')!.getAttribute('src')).toBe(profile('/bicep').avatarUrl);
		expect(document.querySelector('li [aria-label="verified"]')).toBeTruthy();
		// No SoundCloud tab is needed to see it again.
		expect(addButton()!.getAttribute('aria-label')).toBe('refresh 2 artists from this tab');
	});

	it('files artists from an untitled page of no particular kind under “a playlist”', async () => {
		chromeAnswering({ page: extraction({ tracks: TRACKS, title: '', pageType: 'other' }) });
		await load();
		await addTab();
		expect(stored().map((one) => one.from)).toEqual(['a playlist', 'a playlist']);
	});

	it('says so when the tab could not be read, and keeps the artists', async () => {
		chromeAnswering({ lookup: new Error('Could not establish connection') });
		await load();
		await addTab();
		expect(status()!.textContent).toMatch(/couldn’t read their socials\. can’t reach the page/);
		expect(rowText()).toEqual(['Bicep not loaded yet', 'Four Tet not loaded yet']);
		expect(incompleteLine()).toBe(`2 artists without socials yet. ${LOAD_HINT}`);
		expect(meta()).toBe('2 artists · 0 with socials · 2 not loaded yet');
	});

	// A content script from before this answer's shape: a tab to reload.
	it('says so when the answer cannot be read at all', async () => {
		chromeAnswering({ lookup: { ok: false, error: 'boom' } });
		await load();
		await addTab();
		expect(status()!.textContent).toMatch(
			/couldn’t read their socials\. couldn’t read them\. reload the SoundCloud tab/
		);
	});

	it('refreshes rather than duplicates, and asks only about who is not filled in', async () => {
		chromeAnswering({
			lookup: answer([profile('/fourtet')]),
			...held([entry('/bicep', { profile: { ...profile('/bicep'), at: 1 } })])
		});
		await load();
		// Everybody on the list is filled in already, so opening asks nothing.
		expect(lookups()).toEqual([]);
		expect(incompleteLine()).toBe('');
		expect(addButton()!.getAttribute('aria-label')).toBe('add 1 artist from this tab');
		await addTab();
		expect(text()).toMatch(/added 1 artist, refreshed 1 artist from My Playlist/);
		expect(lookups()).toEqual([{ type: 'GET_ARTIST_PROFILES', permalinks: ['/fourtet'] }]);
		expect(stored()).toHaveLength(2);
		expect(rowText()[1]).toMatch(/^Four Tet Belfast, GB/);
		expect(incompleteLine()).toBe('');
	});

	it('says when the list is full, and adds nothing', async () => {
		chromeAnswering(
			// Each answered by an older lookup, so there is nobody to fill in. A
			// search hides every row: two thousand of them are not what this test
			// is about, and the list holds them either way.
			held(
				Array.from({ length: 2000 }, (_, n) => entry(`/a${n}`, { profile: { unavailable: true, at: 1 } })),
				{ view: { open: 'artists', panels: { artists: { query: 'zzzz' } } } }
			)
		);
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
		expect(incompleteLine()).toBe('2 artists without socials yet. open a SoundCloud tab to fill in socials.');
		// Not asked and refused: not asked at all, so there is no failure to report.
		expect(text()).not.toMatch(/couldn’t read their socials/);
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

describe('filling in as it opens', () => {
	it('asks any SoundCloud page about everyone not filled in, and draws what it had loaded', async () => {
		chromeAnswering({
			tab: { id: 7, url: 'https://soundcloud.com/stream' },
			page: extraction({ pageType: 'other', tracks: [] }),
			lookup: answer([profile('/a'), profile('/b', { links: [], partial: true })]),
			...held([
				entry('/a'),
				entry('/b'),
				entry('/c', { profile: { unavailable: true, at: 1 } }),
				entry('/d', { profile: { ...profile('/d', { links: [], partial: true }), at: 1 } }),
				entry('/e', { profile: { ...profile('/e'), at: 1 } })
			])
		});
		await load();
		expect(text()).toMatch(/that’s not a playlist, likes or track page/);
		// Neither the older lookup's "no profile" nor a whole profile has more to load.
		expect(lookups()).toEqual([{ type: 'GET_ARTIST_PROFILES', permalinks: ['/a', '/b', '/d'] }]);
		expect(rowText()).toEqual([
			'a Belfast, GB · 1.2K followers · 1 link IG',
			'b Belfast, GB · 1.2K followers · links not loaded yet',
			'c no public profile',
			'd Belfast, GB · 1.2K followers · links not loaded yet',
			'e Belfast, GB · 1.2K followers · 1 link IG'
		]);
		// A track names its uploader in short; the rest comes with their page.
		expect(lineTitle('/b')).toBe(`Belfast, GB · 1.2K followers · links not loaded yet. ${LOAD_HINT}`);
		expect(lineTitle('/c')).toBe('no public profile');
		expect(incompleteLine()).toBe(`2 artists without socials yet. ${LOAD_HINT}`);
		expect(meta()).toBe('5 artists · 2 with socials');
		expect(await sessionProfiles()).toMatchObject({ '/a': { city: 'Belfast' }, '/b': { partial: true } });
	});

	it('says the bio is still to load in an opened row', async () => {
		chromeAnswering(held([entry('/mid', { name: 'Mid', profile: { ...profile('/mid', { partial: true }), at: 1 } })]));
		await load();
		await click(chevron('Mid'));
		expect(labelled('bio: not loaded yet', HTMLButtonElement).disabled).toBe(true);
	});

	it('says so when the tab could not be read', async () => {
		chromeAnswering({ storage: { artists: [entry('/a')] }, lookup: new Error('Could not establish connection') });
		await load();
		expect(lookups()).toHaveLength(1);
		expect(status()!.textContent).toMatch(/couldn’t read their socials\. can’t reach the page/);
		expect(rowText()).toEqual(['a not loaded yet']);
	});

	it('asks nothing of a tab that is not SoundCloud, and says what to open', async () => {
		chromeAnswering({ tab: { id: 1, url: 'https://example.com/' }, storage: { artists: [entry('/a')] } });
		await load();
		expect(lookups()).toEqual([]);
		expect(incompleteLine()).toBe('1 artist without socials yet. open a SoundCloud tab to fill in socials.');
		expect(lineTitle('/a')).toBe(`not loaded yet. ${LOAD_HINT}`);
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
		chromeAnswering(held(THREE));
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
		// Five on the profile and one in the bio; the email address in the bio
		// is not a link.
		expect(mid.textContent).toMatch(/99 followers · 6 links/);
		expect(mid.querySelectorAll('[aria-label*=": open "]')).toHaveLength(3);
		expect(mid.textContent).toMatch(/\+3/);
		expect(mid.querySelector('[title="no picture"]')).toBeTruthy();
		expect(mid.querySelector('img')).toBe(null);
	});

	it('opens a link in a tab behind the popup, or in front when asked', async () => {
		await load();
		await click(byLabel('IG: open Instagram'));
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: 'https://instagram.com/mid', active: false });

		await click(document.getElementById('background'));
		expect(localStorage.getItem('pref:background')).toBe('0');
		await click(byLabel('IG: open Instagram'));
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

	it('draws a long list a screenful at a time, and what a search hid the same way', async ({ signal }) => {
		vi.useFakeTimers();
		chromeAnswering({ storage: { artists: Array.from({ length: 200 }, (_, n) => entry(`/a${n}`)) } });
		await load();
		expect(rows()).toHaveLength(60);

		// jsdom's frames come from a setInterval, which the fake timers hold, so
		// each 60ms step is the same three frames every run.
		await stepFrames(6, signal);
		expect(rows()).toHaveLength(200);

		const search = labelled('search the artists', HTMLInputElement);
		await type(search, 'a199');
		expect(rows()).toHaveLength(1);
		const [stayed] = rows();

		// Clearing it puts back a frame's worth, beside the row that stayed.
		await type(search, '');
		expect(rows()).toHaveLength(41);
		expect(rows()).toContain(stayed);
		await stepFrames(6, signal);
		expect(rows()).toHaveLength(200);
		expect(new Set(rows().map((li) => li.dataset.permalink)).size).toBe(200);
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
		chromeAnswering(held([MID, entry('/other')]));
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

		// Each field is named by what it says, so a screen reader reads the value;
		// that a click copies it is the description.
		const instagram = field('https://instagram.com/mid')!;
		expect(instagram.hasAttribute('aria-label')).toBe(false);
		await click(instagram);
		expect(clipboard.writeText).toHaveBeenCalledWith('https://instagram.com/mid');
		expect(text()).toMatch(/copied the Instagram/);
		expect(instagram.title).toMatch(/the arrow opens it/);
		expect(field('hello')?.title).toBe('click to copy');

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
		const beatport = labelled('BP: search Beatport', HTMLElement);
		expect(beatport.title).toBe('search Beatport for “Mid”');
		await click(beatport);
		expect(chrome.tabs.create).toHaveBeenLastCalledWith({
			url: 'https://www.beatport.com/search?q=Mid',
			active: false
		});
		expect(byLabel('bc: search Bandcamp')).toBeTruthy();
	});

	it('takes the artist off the list', async () => {
		await load();
		await click(chevron('Mid'));
		await click(byLabel('take Mid off the list'));
		expect(text()).toMatch(/took Mid off the list/);
		expect(rowText()).toEqual(['other not loaded yet']);
		expect(stored().map((one) => one.permalink)).toEqual(['/other']);
	});

	// A state file can restore a profile as little as `{ at }`: a follower count
	// it does not carry is not known, and the row says and opens as for null.
	it('opens a restored profile that carries no follower count', async () => {
		chromeAnswering(held([entry('/bare', { name: 'Bare', profile: { at: 1 } })]));
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
