// Every panel, in every state a test can put it in, through an accessibility
// audit (test/helpers/a11y.ts).
//
// The panels are icon-heavy and dense: most of what a row offers is a button
// with a picture in it, and most of what it says is said by colour. That works
// only if each of those carries a name and a state a screen reader can read,
// and nothing else in the suite would notice if one stopped doing so —
// `byLabel('mark downloaded')` fails loudly, but a *new* button with no
// label at all fails nowhere.
import { describe, it, expect } from 'vitest';
import { audit, accessibleName, focusable } from './helpers/a11y.ts';
import {
	addPage,
	click,
	extraction,
	mountApp,
	mountPanel,
	openQueue,
	panelChrome,
	queueButton,
	settle,
	track,
	type
} from './helpers/panel.ts';

/** A state the audit is run in, as STATES and SHEET_STATES below list them. */
type State = [
	name: string,
	setUp: () => void,
	act?: () => Promise<void>,
	/** How many controls it has to put on screen, at least. */
	minControls?: number
];

const TRACKS = [
	track('Bicep', 'Glue'),
	track('Four Tet', 'Baby'),
	track('Floating Points', 'LesAlpx', { artworkUrl: '' })
];

const details = (extra = {}) => ({
	id: 1,
	permalink: '/x/y',
	downloadable: false,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	at: 1,
	...extra
});

const withDetails = () => [
	{
		...track('Bicep', 'Glue'),
		liked: true,
		reposted: false,
		details: details({ permalink: '/bicep/glue', downloadable: true, genre: 'Techno' })
	},
	{
		// A badge tile: nothing on the page said whether it is liked.
		...track('Four Tet', 'Baby'),
		liked: null,
		reposted: null,
		details: details({ permalink: '/fourtet/baby', purchaseUrl: 'https://hypeddit.com/x' })
	},
	{ ...track('Nobody', 'Nothing'), details: details({ permalink: '/nobody/nothing' }) },
	{ ...track('Unknown', 'Unchecked') }
];

// The artists tool's list, one artist in each state a row can be in: one
// SoundCloud described (links, a picture, a verified mark), one it had no
// profile for, and one nobody has asked about.
const ARTISTS = [
	{
		permalink: '/bicep',
		name: 'Bicep',
		tracks: [{ permalink: '/bicep/glue', title: 'Glue' }],
		profile: {
			id: 1,
			permalink: '/bicep',
			username: 'Bicep',
			fullName: '',
			city: 'Belfast',
			country: 'GB',
			followers: 1234,
			trackCount: 3,
			verified: true,
			avatarUrl: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
			bio: 'bookings: agent@example.com',
			links: [
				{ network: 'instagram', title: '', url: 'https://www.instagram.com/bicepmusic/' },
				{ network: 'personal', title: 'site', url: 'https://feelmybicep.com' }
			],
			at: 1
		},
		from: 'My Playlist',
		at: 1
	},
	{
		permalink: '/nobody',
		name: 'Nobody',
		tracks: [{ permalink: '/nobody/nothing', title: 'Nothing' }],
		profile: { unavailable: true, at: 1 },
		from: 'the session queue',
		at: 1
	},
	{ permalink: '/unknown', name: '', tracks: [], profile: null, from: '', at: 1 }
];

const clean = () => expect(audit()).toEqual([]);

// Every state each panel can be in that a test can reach, as
// [name, what to set up, what to do once it is mounted].
const STATES: [id: string, states: State[]][] = [
	[
		'soundcloud-playlist-to-text',
		[
			['no SoundCloud tab', () => panelChrome({ tab: null })],
			['an empty page', () => panelChrome({ response: extraction({ tracks: [] }) })],
			['tracks', () => panelChrome({ response: extraction({ tracks: TRACKS }) })],
			[
				'tracks, with the format menu open',
				() => panelChrome({ response: extraction({ tracks: TRACKS }) }),
				async () => {
					await click(document.querySelector('[aria-label="output format"]'));
				}
			]
		]
	],
	[
		'manager',
		[
			// The tool keeps a list of its own, so its opening state is a panel with
			// nothing on it — which is a state a screen reader has to be able to read
			// its way out of, on any tab at all.
			['no SoundCloud tab, and nothing on the list', () => panelChrome({ tab: null })],
			[
				'a page with nothing on it',
				() => panelChrome({ response: extraction({ tracks: [], lookupFailed: 0, resumed: false, error: '' }) })
			],
			[
				'a page to add, not yet added',
				() =>
					panelChrome({
						response: extraction({ tracks: withDetails(), lookupFailed: 0, resumed: false, error: '' })
					})
			],
			[
				'links of every kind',
				() =>
					panelChrome({
						response: extraction({ tracks: withDetails(), lookupFailed: 1, resumed: false, error: '' })
					}),
				addPage
			],
			[
				'links of every kind, one of them marked',
				() =>
					panelChrome({
						response: extraction({ tracks: withDetails(), lookupFailed: 0, resumed: false, error: '' }),
						storage: { downloaded: { '/bicep/glue': '2026-01-01T00:00:00.000Z' } }
					}),
				addPage
			],
			[
				'links of every kind, with a batch being selected',
				() =>
					panelChrome({
						response: extraction({ tracks: withDetails(), lookupFailed: 0, resumed: false, error: '' })
					}),
				async () => {
					await addPage();
					// Select in the header, then the first row's tick box: the batch bar
					// and a ticked row are both states of their own.
					await click(document.querySelector('[aria-label="select several tracks"]'));
					await click(document.querySelector('li [role="checkbox"]'));
					expect(document.body.textContent).toContain('1 track ticked');
				}
			],
			[
				// Two rows opened: one whose every field is known, one whose like and
				// repost state nothing on the page said — the fields that read
				// "unknown" are buttons that cannot be pressed, and a disabled button
				// still has to be named.
				'links of every kind, with rows opened',
				() =>
					panelChrome({
						response: extraction({ tracks: withDetails(), lookupFailed: 0, resumed: false, error: '' })
					}),
				async () => {
					await addPage();

					for (const chevron of [...document.querySelectorAll('li [aria-expanded]')].slice(0, 2)) {
						await click(chevron);
					}

					expect(document.querySelectorAll('li dl').length).toBe(2);
				}
			],
			[
				'links of every kind, filtered to none',
				() =>
					panelChrome({
						response: extraction({ tracks: withDetails(), lookupFailed: 0, resumed: false, error: '' })
					}),
				async () => {
					await addPage();
					await type(document.querySelector('[aria-label="search the list"]'), 'zzzzzz');
				}
			],
			[
				'a lookup that failed outright',
				() =>
					panelChrome({
						response: extraction({
							tracks: withDetails(),
							lookupFailed: 0,
							resumed: false,
							error: 'unauthorized'
						})
					}),
				addPage
			]
		]
	],
	[
		'artists',
		[
			['no SoundCloud tab, and nobody on the list', () => panelChrome({ tab: null })],
			['a page to add, not yet added', () => panelChrome({ response: extraction({ tracks: TRACKS }) })],
			[
				// Added on a page whose every answer is the page itself, which is a
				// lookup that could not be read: the artists are listed, unlooked-up,
				// under the note that says so.
				'a page added, whose lookup could not be read',
				() => panelChrome({ response: extraction({ tracks: TRACKS }) }),
				async () => {
					await click(document.querySelector('[aria-label$="from this tab"]'));
					expect(document.body.textContent).toContain('not looked up');
				}
			],
			[
				'artists of every kind',
				() => panelChrome({ response: extraction({ tracks: TRACKS }), storage: { artists: ARTISTS } })
			],
			[
				'artists of every kind, with rows opened',
				() => panelChrome({ response: extraction({ tracks: TRACKS }), storage: { artists: ARTISTS } }),
				async () => {
					for (const chevron of document.querySelectorAll('li [aria-expanded]')) await click(chevron);
					expect(document.querySelectorAll('li dl').length).toBe(3);
				}
			],
			[
				'artists of every kind, filtered to none',
				() => panelChrome({ response: extraction({ tracks: TRACKS }), storage: { artists: ARTISTS } }),
				async () => {
					await type(document.querySelector('[aria-label="search the artists"]'), 'zzzzzz');
				}
			],
			[
				'artists of every kind, with the sort menu open',
				() => panelChrome({ response: extraction({ tracks: TRACKS }), storage: { artists: ARTISTS } }),
				async () => {
					await click(document.querySelector('[aria-label="sort order"]'));
				}
			]
		]
	],
	[
		'third-party-downloads',
		[
			['no SoundCloud tab', () => panelChrome({ tab: null })],
			[
				'gate links found on the page',
				() =>
					panelChrome({
						response: extraction({
							tracks: [
								{
									...track('Artist', 'One'),
									details: details({ permalink: '/artist/one', purchaseUrl: 'https://hypeddit.com/886rp1' })
								},
								{
									...track('Artist', 'Two'),
									details: details({ permalink: '/artist/two', purchaseUrl: 'https://laylo.com/x/y' })
								}
							],
							lookupFailed: 0,
							resumed: false,
							error: ''
						})
					})
			]
		]
	]
];

describe.each(STATES)('%s', (id, states) => {
	it.each(states)('is readable without sight: %s', async (_name, setUp, act, minControls = 1) => {
		setUp();
		await mountPanel(id);

		if (act) await act();
		await settle();
		// An audit of a panel that drew nothing would pass without saying
		// anything; every state here has to have put something on screen.
		expect(document.body.textContent.trim().length).toBeGreaterThan(20);
		expect(focusable().length).toBeGreaterThanOrEqual(minControls);
		clean();
	});
});

describe('the queue sheet', () => {
	const SHEET_STATES: State[] = [
		['an empty queue', () => panelChrome()],
		['a queue with tracks', () => panelChrome({ session: { queue: TRACKS } })],
		[
			'a queue with a marked track',
			() =>
				panelChrome({
					session: { queue: TRACKS },
					storage: { downloaded: { [TRACKS[0].permalink]: '2026-01-01T00:00:00.000Z' } }
				})
		]
	];

	it.each(SHEET_STATES)('is readable without sight: %s', async (_name, setUp) => {
		setUp();
		await mountApp();
		await openQueue();
		expect(document.body.textContent.trim().length).toBeGreaterThan(20);
		// Even over an empty queue there is a way out of it.
		expect(focusable().length).toBeGreaterThanOrEqual(1);
		clean();
	});

	// A sheet that can be tabbed out of behind is not a sheet: while it is up,
	// the only controls a keyboard reaches are its own.
	it('takes the keyboard away from the view it covers', async () => {
		panelChrome({ session: { queue: TRACKS } });
		await mountApp();
		const behind = focusable();
		expect(behind.length).toBeGreaterThan(1);

		const sheet = await openQueue();
		const reachable = focusable();
		expect(reachable.length).toBeGreaterThan(0);

		for (const el of reachable) expect(sheet!.contains(el), el.outerHTML.slice(0, 80)).toBe(true);
	});

	// Icon-only and in every header in the extension: if it lost its name there
	// would be no way to tell what it was, and no other test would notice.
	it('names the button that opens it, and says whether it is up', async () => {
		panelChrome({ session: { queue: TRACKS } });
		await mountApp();
		const opener = queueButton()!;
		expect(accessibleName(opener)).toBe('session queue: 3 tracks');
		expect(opener.getAttribute('aria-expanded')).toBe('false');

		await openQueue();
		expect(queueButton()!.getAttribute('aria-expanded')).toBe('true');
		clean();
	});
});

describe('the popup as a whole', () => {
	it('names every tool in the menu, and puts them in one list', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountApp();
		clean();

		const menu = document.querySelectorAll('nav button, main button');
		expect(menu.length).toBeGreaterThan(0);

		for (const item of menu) expect(accessibleName(item)).not.toBe('');
	});

	it('keeps every control reachable by keyboard, in the order it is drawn', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountApp();

		const reachable = focusable();
		expect(reachable.length).toBeGreaterThan(1);

		// Document order is tab order: nothing re-orders itself with tabindex.
		for (const el of reachable) expect(Number(el.getAttribute('tabindex') ?? 0)).toBe(0);
	});

	it('stays readable inside a tool, and on the way back out', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountApp();

		const open = [...document.querySelectorAll('button')].find((b) => b.textContent.includes('playlist to text'));
		await click(open);
		clean();
		expect(accessibleName(document.querySelector('[aria-label="back to all tools"]')!)).toBe('back to all tools');

		await click(document.querySelector('[aria-label="back to all tools"]'));
		clean();
	});

	it('says which theme the toggle would switch to, in both themes', async () => {
		panelChrome();
		await mountApp();
		const toggle = () => document.querySelector('[aria-label^="theme:"]');
		const first = toggle()!.getAttribute('aria-label');
		await click(toggle());
		expect(toggle()!.getAttribute('aria-label')).not.toBe(first);
		clean();
	});
});

describe('the audit itself', () => {
	// A check that never fails is a check that is not running.
	it('catches the mistakes it is there to catch', () => {
		document.body.innerHTML = `
      <button><svg></svg></button>
      <div id="dup"></div><div id="dup"></div>
      <input aria-labelledby="nowhere">
      <label for="missing">Name</label>
      <a href="#x" tabindex="3">up front</a>
      <div aria-hidden="true"><button>hidden but reachable</button></div>
      <img src="x.jpg">
      <div aria-pressed="true">not a button</div>
      <button aria-pressed="yes">odd</button>
      <button aria-disabled="true">still clickable</button>`;
		const problems = audit();
		expect(problems).toEqual(
			expect.arrayContaining([
				expect.stringContaining('has no accessible name'),
				expect.stringContaining('reuses the id "dup"'),
				expect.stringContaining('aria-labelledby points at "nowhere"'),
				expect.stringContaining('labels "missing"'),
				expect.stringContaining('has a positive tabindex'),
				expect.stringContaining('is focusable inside an aria-hidden subtree'),
				expect.stringContaining('has no alt attribute'),
				expect.stringContaining('has aria-pressed but its role is "none"'),
				expect.stringContaining('has aria-pressed="yes"'),
				expect.stringContaining('is aria-disabled but still focusable')
			])
		);
	});

	// bits-ui marks a closed tooltip's or select's trigger data-state="closed",
	// just as it marks the closed content the trigger opens. The content is
	// skipped; the trigger — nearly always an icon-only button — is not.
	it('audits the trigger of a closed tooltip or select, and skips only what it opens', () => {
		document.body.innerHTML = `
      <button class="tip" data-state="closed" data-tooltip-trigger><svg></svg></button>
      <div data-state="closed" data-tooltip-trigger>
        <button class="wrapped"><svg></svg></button>
      </div>
      <button class="pick" data-state="closed" aria-haspopup="listbox" aria-expanded="false">
        <svg></svg>
      </button>
      <div role="listbox" data-state="closed"><div role="option"></div></div>`;
		expect(audit()).toEqual([
			'<button.tip> has no accessible name',
			'<button.wrapped> has no accessible name',
			'<button.pick> has no accessible name'
		]);
	});

	it('passes markup that is right', () => {
		document.body.innerHTML = `
      <button aria-label="Save">🖫</button>
      <label for="q">Search</label><input id="q">
      <img src="x.jpg" alt="Cover art">
      <img src="y.jpg" aria-hidden="true">
      <button aria-pressed="false">Toggle</button>
      <button disabled aria-disabled="true">Off</button>`;
		expect(audit()).toEqual([]);
	});
});
