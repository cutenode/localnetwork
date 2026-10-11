// src/tools/third-party-downloads/Panel.svelte — the third-party downloader,
// against the real downloaded-store.ts. The sites themselves are a stubbed
// `fetch`: a gate page as HTML, then whatever endpoints walking it goes
// through. What each site's walk looks like is tested in test/tools/gates.test.ts;
// what is here is the panel around them — where the links come from, what a row
// says as it runs, and which of "saved" and "somewhere to open" it ends on.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { flush } from '../helpers/flush.ts';
import {
	button,
	click,
	extraction,
	mountApp,
	mountPanel,
	openSettings,
	panelChrome,
	reopenPanel,
	settingsButton,
	settingsSheet,
	settle,
	text,
	track,
	type
} from '../helpers/panel.ts';
import type { ChromeMock, Json } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

/** What one site answers a request with: a stubbed response, or a promise of one. */
type Route = (options: RequestInit) => Answer | Promise<Answer>;

const load = () => mountPanel('third-party-downloads');

// The whole popup, opened on this tool, for the tests whose gesture leads out
// of the panel: the address and the name a gate asks for are set in the
// settings sheet now, and only the shell draws that (src/App.svelte).
const loadInPopup = async () => {
	await mountApp();
	await click(button('third-party downloader'));
};

/**
 * Sets one of the Downloader's settings where the settings sheet would.
 *
 * They are the popup's own options (src/lib/downloader.svelte.js), read out of
 * localStorage as the module graph is built — which a mount does — so a test
 * that needs one already set writes it before mounting, rather than opening
 * the sheet it is not there to test.
 */
const alreadySet = (values: Record<string, string>) => {
	for (const [key, value] of Object.entries(values)) localStorage.setItem(`pref:${key}`, value);
};

const HYPE = 'https://hypeddit.com/886rp1';

const HYPE2 = 'https://hypeddit.com/artist/second-track';

const DROP = 'https://droploud.com/gate/abc';

const LAYLO = 'https://laylo.com/hairitage/9PWcf';

// --- the sites, stubbed ------------------------------------------------------

const gateHtml = (uid = '886rp1', steps = 'sc') => `<html><head>
<meta name="csrf-token" content="CSRF">
</head><body>
<input id="current_download_file_listner" value="${uid}">
<input id="wrndk" value="1381436x5194282">
<input id="gvt" value="TOKEN">
<input id="fan_gate_id" value="1381436">
<input id="nwSteps" value="${steps}">
<input id="is_skippable" value="0">
<input id="duration" value="236000">
${
	steps.includes('sp')
		? `<div id="step_sp"><a onclick="PopupCenterDual('https://accounts.spotify.com/authorize?state=${uid}')">Connect</a></div>`
		: ''
}
</body></html>`;

const signed = (name = 'KAKU - FE!N (Remix).wav') =>
	`https://gates.s3.amazonaws.com/886rp1?response-content-disposition=${encodeURIComponent(
		`attachment; filename="${name}"`
	)}`;

const droploudHtml = (title = 'Front to Back') =>
	`<script>self.__next_f.push([1,${JSON.stringify(
		`a:{"gateData":{"track":{"id":"abc","title":${JSON.stringify(
			title
		)}},"artist":{"display_name":"D!lemma"},"steps":[{"id":"sc_follow","type":"soundcloud_follow"}]}}\n`
	)}])</script>`;

const laylaHtml = `<script>fetch('https://d21i0hc4hl3bvt.cloudfront.net/hairitage/9PWcf.json')</script>`;

/** A response with `body` as its text: a page, or what a JSON endpoint sent. */
const res = (body: string, { ok = true, status = 200 } = {}) => ({
	ok,
	status,
	url: '',
	redirected: false,
	body: { cancel: vi.fn<() => void>() },
	text: async () => body,
	json: async (): Promise<Json> => JSON.parse(body)
});

/** A JSON endpoint's response. */
const json = (value: Json, options?: { ok?: boolean; status?: number }) => res(JSON.stringify(value), options);

type Answer = ReturnType<typeof res>;

/** A fetch answering for every site the panel might reach, overridable. */
function net(over: Record<string, Route> = {}) {
	const routes = Object.entries<Route>({
		'hypeddit.com/886rp1': () => res(gateHtml()),
		'hypeddit.com/artist/second-track': () => res(gateHtml('second')),
		'/gate/ge': () => json({ ok: 1 }),
		'/gate/download/ul': () => json({ download_status: true, URL: signed() }),
		'droploud.com/gate/abc': () => res(droploudHtml()),
		'api.droploud.com': () => json({ download_url: 'https://cdn.test/Front%20to%20Back.wav' }),
		'laylo.com/hairitage': () => res(laylaHtml),
		d21i0hc4hl3bvt: () => json({ title: 'REMIXES', rsvpOptions: { email: 1 }, user: {} }),
		d3oyaxbt9vo0fg: () => json({ id: 'a1', displayName: 'Hairitage' }),
		...over
	});

	const calls: { url: string; options: RequestInit; body: string }[] = [];
	vi.stubGlobal(
		'fetch',
		vi.fn<(url: string, options?: RequestInit & { body?: string }) => Promise<Answer>>(async (url, options = {}) => {
			calls.push({ url, options, body: options.body ?? '' });
			const route = routes.find(([match]) => url.includes(match));

			if (!route) throw new Error(`nothing stubbed for ${url}`);

			return route[1](options);
		})
	);

	return calls;
}

// --- reading the panel -------------------------------------------------------

const clean = (node: Element) => node.textContent.replace(/\s+/g, ' ').trim();

// A row's first paragraph is its title; everything after it is what the row
// says about itself — the state line, and, on a row that failed, the band the
// card opens underneath to give the message the full width. Read as one
// string, the way it reads on screen.
const rows = () =>
	[...document.querySelectorAll('li')].map((li) => {
		const [title, ...said] = li.querySelectorAll('p');

		return {
			title: clean(title),
			state: said.map(clean).filter(Boolean).join(' ')
		};
	});

const rowButtons = (label: string) =>
	[...document.querySelectorAll<HTMLButtonElement>('li button')].filter((b) => clean(b) === label);

const allButton = () => [...document.querySelectorAll('header button')].at(-1);

/** The Downloaded check button of each listed row, in list order. */
const markButtons = () => [...document.querySelectorAll('li button[aria-label="mark downloaded"]')];

// By its exact text: "add links" would otherwise also match the drawer's own
// trigger, "add links by hand".
const exactButton = (label: string) => [...document.querySelectorAll('button')].find((b) => clean(b) === label);

const meta = () => document.querySelector('header p')?.textContent.trim() ?? '';

// The drawer is the paste box and nothing else: what a gate asks for moved to
// the settings sheet.
const drawer = async () => {
	const trigger = [...document.querySelectorAll('button')].find((b) => /add links by hand/.test(clean(b)));

	if (trigger!.getAttribute('data-state') !== 'open') await click(trigger);
};

const field = (id: string) => document.getElementById(id);

// A SoundCloud answer whose tracks carry purchase links.
const page = (...urls: string[]) =>
	extraction({
		title: 'My Playlist',
		tracks: urls.map((url, i) =>
			track('Artist', `Track ${i + 1}`, {
				details: {
					downloadable: false,
					purchaseUrl: url,
					purchaseTitle: 'Free Download'
				}
			})
		)
	});

beforeEach(() => net());

// --- where the links come from -----------------------------------------------

describe('collecting links', () => {
	it('takes every link on a SoundCloud page that one of the sites claims', async () => {
		panelChrome({
			response: page(HYPE, 'https://bandcamp.com/nope', DROP, LAYLO)
		});
		await load();

		expect(rows().map((row) => row.title)).toEqual(['Artist — Track 1', 'Artist — Track 3', 'Artist — Track 4']);
		expect(rows().map((row) => row.state)).toEqual(['Hypeddit', 'Droploud', 'Laylo']);
		expect(meta()).toBe('3 links · My Playlist');
	});

	it('unwraps SoundCloud’s redirector on the way', async () => {
		panelChrome({
			response: page(`https://gate.sc?url=${encodeURIComponent(HYPE)}&token=t`)
		});
		await load();
		expect(rows()).toHaveLength(1);
	});

	it('lists nothing twice, however many tracks share a link', async () => {
		panelChrome({ response: page(HYPE, HYPE) });
		await load();
		expect(rows()).toHaveLength(1);
	});

	it('takes the gate in the active tab when the tab is one', async () => {
		panelChrome({ tab: { id: 3, url: DROP } });
		await load();
		expect(rows()).toEqual([{ title: 'abc', state: 'Droploud' }]);
		expect(meta()).toBe('1 link · this tab');
	});

	it('says which sites it knows when a page has none of them', async () => {
		panelChrome({ response: page('https://bandcamp.com/one') });
		await load();
		expect(text()).toMatch(/nothing on My Playlist points at a site this knows/);
		expect(text()).toMatch(/Hypeddit, ToneDen, Droploud/);
	});

	it('says what to open when the tab is neither', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		await load();
		expect(text()).toMatch(/open a gate link or a SoundCloud page in this tab/);
	});

	it('says to reload a page it cannot reach', async () => {
		panelChrome({ response: new Error('no receiver') });
		await load();
		expect(text()).toMatch(/can’t reach the page/);
	});

	it('says to reload a page that would not extract', async () => {
		panelChrome({ response: extraction({ ok: false }) });
		await load();
		expect(text()).toMatch(/couldn’t read the page/);
	});

	// The content script hands over what it has learned in batches rather than
	// all at the end, so a track whose purchase link was still unknown when the
	// list went up joins the list as its answer lands.
	it('lists a track whose link arrives after the page did', async () => {
		panelChrome({
			response: extraction({
				tracks: [track('Artist', 'Track 1'), track('Artist', 'Track 2', { details: { purchaseUrl: HYPE } })]
			})
		});

		await load();
		expect(rows()).toHaveLength(1);

		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			details: [{ permalink: '/artist/track1', purchaseUrl: DROP }]
		});
		await settle();
		expect(rows().map((row) => row.state)).toEqual(['Hypeddit', 'Droploud']);

		// A second answer about a track that already has one changes nothing, and
		// neither does anything that is not an answer.
		await chrome.testSendMessage({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			details: [{ permalink: '/artist/track1', purchaseUrl: LAYLO }]
		});
		await chrome.testSendMessage({ type: 'DOWNLOAD_DETAILS_PROGRESS', details: [] });
		await chrome.testSendMessage({ type: 'SOMETHING_ELSE', details: [{ permalink: '/a/b' }] });
		await chrome.testSendMessage(null);
		await settle();
		expect(rows()).toHaveLength(2);
	});
});

describe('the session queue as the source', () => {
	const queued = {
		queue: [
			{
				permalink: '/artist/one',
				title: 'One',
				artist: 'Artist',
				details: { purchaseUrl: HYPE }
			},
			{ permalink: '/artist/two', title: 'Two', artist: 'Artist', details: {} }
		]
	};

	it('takes the links of the queued tracks, whatever the tab is', async () => {
		panelChrome({
			tab: { id: 1, url: 'https://example.com/' },
			session: queued
		});
		await load();
		await click(exactButton('session queue (2)'));

		expect(rows()).toEqual([{ title: 'Artist — One', state: 'Hypeddit in the queue' }]);
		expect(meta()).toBe('1 link · session queue');
	});

	// Choosing the source that is already chosen is not a choice: the list is
	// built from it once, and stays as it was rather than being thrown away and
	// collected again.
	it('does nothing when the source already chosen is picked again', async () => {
		panelChrome({ response: page(HYPE) });
		await load();
		expect(rows()).toHaveLength(1);
		const reads = chrome.tabs.sendMessage.mock.calls.length;

		await click(exactButton('this page'));
		expect(rows()).toHaveLength(1);
		expect(chrome.tabs.sendMessage.mock.calls).toHaveLength(reads);
	});

	it('says when the queue is empty', async () => {
		panelChrome({ session: { queue: [] } });
		await load();
		await click(exactButton('session queue (0)'));
		expect(text()).toMatch(/nothing queued yet/);
	});

	it('says when nothing in the queue links anywhere it knows', async () => {
		panelChrome({
			session: {
				queue: [{ permalink: '/a/one', title: 'One', artist: 'A', details: {} }]
			}
		});
		await load();
		await click(exactButton('session queue (1)'));
		expect(text()).toMatch(/none of the 1 track in the queue point at a site this knows/);
	});
});

describe('the paste box', () => {
	it('picks links out of anything paste-shaped', async () => {
		panelChrome({ tab: null });
		await load();
		await drawer();

		await type(field('paste'), `look: ${HYPE}, and (${DROP}) plus https://example.com/no`);
		await click(exactButton('add links'));

		expect(rows().map((row) => row.state)).toEqual(['Hypeddit', 'Droploud']);
		expect(field('paste')).toHaveProperty('value', '');
		expect(text()).toMatch(/added 2 links/);
	});

	it('says when there is nothing in the text, and when it is all there already', async () => {
		panelChrome({ tab: null });
		await load();
		await drawer();

		await type(field('paste'), 'no links here');
		await click(exactButton('add links'));
		expect(text()).toMatch(/no links this tool knows in that text/);

		await type(field('paste'), HYPE);
		await click(exactButton('add links'));
		await type(field('paste'), HYPE);
		await click(exactButton('add links'));
		expect(text()).toMatch(/already listed: that link/);
	});
});

// --- running -----------------------------------------------------------------

describe('walking a link', () => {
	it('unlocks a gate, saves the file under the site’s folder, and marks the track', async () => {
		panelChrome({ response: page(HYPE) });
		const { downloaded } = await load();

		await click(rowButtons('get')[0]);

		expect(chrome.downloads.download).toHaveBeenCalledWith({
			url: signed(),
			filename: 'Hypeddit/KAKU - FE!N (Remix).wav'
		});
		// The row takes the name the artist gave the file.
		expect(rows()).toEqual([{ title: 'KAKU - FE!N (Remix).wav', state: 'Hypeddit saved' }]);
		expect(downloaded.has('/artist/track1')).toBe(true);
		expect(meta()).toBe('1 link · My Playlist · 1 saved');
	});

	it('sends each site to its own folder', async () => {
		panelChrome({ response: page(DROP) });
		await load();
		await click(rowButtons('get')[0]);
		expect(chrome.downloads.download).toHaveBeenCalledWith({
			url: 'https://cdn.test/Front%20to%20Back.wav',
			filename: 'Droploud/Front to Back.wav'
		});
		expect(rows()[0].title).toBe('Front to Back.wav');
	});

	it('leaves the track unmarked when that setting is off', async () => {
		alreadySet({ mark: '0' });
		panelChrome({ response: page(HYPE) });
		const { downloaded } = await load();

		await click(rowButtons('get')[0]);
		expect(downloaded.has('/artist/track1')).toBe(false);
		expect(rows()[0].state).toMatch(/saved/);
	});

	it('offers somewhere to open rather than reporting a failure it did not have', async () => {
		panelChrome({ response: page(LAYLO) });
		await load();

		await click(rowButtons('get')[0]);
		expect(rows()).toEqual([
			{
				title: 'REMIXES',
				state: expect.stringMatching(/^Laylo Laylo is a notify-me page, not a gate/)
			}
		]);
		expect(chrome.downloads.download).not.toHaveBeenCalled();
		expect(meta()).toBe('1 link · My Playlist · 1 to open');

		await click(rowButtons('open')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: LAYLO,
			active: true
		});
	});

	// A Laylo drop is a mailing list rather than a gate, so the walk is the
	// signup: the address from settings is left with the artist, and the row
	// says whose list it is on now rather than claiming a file it never got.
	const signable = {
		title: 'REMIXES',
		productId: 'p1',
		rsvpOptions: { email: 1 },
		user: { id: 'a1' }
	};

	it('signs up for a Laylo drop with the address settings holds', async () => {
		alreadySet({ email: 'me@example.com' });
		panelChrome({ response: page(LAYLO) });

		const calls = net({
			d21i0hc4hl3bvt: () => json(signable),
			'events.laylo.com': () => json({ success: true })
		});

		const { downloaded } = await load();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/signed me@example.com up — Hairitage sends the link out/);
		expect(meta()).toBe('1 link · My Playlist');

		const signup = JSON.parse(calls.find((call) => call.url.includes('events.laylo'))!.body);
		expect(signup.payload).toMatchObject({
			productId: 'p1',
			creatorId: 'a1',
			email: 'me@example.com'
		});
		// Signed up is not downloaded: no file was fetched, and nothing is marked.
		expect(chrome.downloads.download).not.toHaveBeenCalled();
		expect(downloaded.has('/artist/track1')).toBe(false);
		// The address is left, and leaving it twice is not a retry worth offering:
		// the row keeps only the drop, the way a row that ended on a page does.
		expect(rowButtons('get')).toHaveLength(0);
		await click(rowButtons('open')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({ url: LAYLO, active: true });
	});

	it('sends a drop that wants an address to settings, not to a tab', async () => {
		panelChrome({ response: page(LAYLO) });
		net({ d21i0hc4hl3bvt: () => json(signable) });
		await loadInPopup();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/add an address in settings/);
		expect(rowButtons('open')).toHaveLength(0);

		await click(rowButtons('add email')[0]);
		expect(chrome.tabs.create).not.toHaveBeenCalled();
		expect(settingsSheet()).not.toBe(null);
		expect(field('email')).toBeTruthy();
	});

	it('sends a drop that wants a name there too, and asks for the name', async () => {
		alreadySet({ email: 'me@example.com' });
		panelChrome({ response: page(LAYLO) });
		net({
			d21i0hc4hl3bvt: () => json({ ...signable, additionalRSVPInputs: [{ fieldName: 'name', required: true }] })
		});
		await loadInPopup();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/asks for a name/);

		await click(rowButtons('add name')[0]);
		expect(settingsSheet()).not.toBe(null);
		expect(field('name')).toBeTruthy();
	});

	it('says on the row why one did not work', async () => {
		panelChrome({ response: page(HYPE) });
		net({ 'hypeddit.com/886rp1': () => res('<html></html>') });
		await load();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toBe('Hypeddit that’s not a Hypeddit gate.');
		expect(meta()).toBe('1 link · My Playlist · 1 failed');
	});

	// A failure is where the work goes back to a person, so the row hands it
	// over: the gate to open, or the settings sheet when what is missing is an
	// address.

	it('offers the gate to open when a walk fails, and queues the track on the way', async () => {
		panelChrome({ response: page(HYPE) });
		net({ '/gate/download/ul': () => json({ download_status: false }) });
		const { queue } = await load();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/the gate wouldn’t unlock/);

		await click(rowButtons('open')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: HYPE,
			active: true
		});
		// The popup closes behind the new tab, so the track goes on a list that
		// outlives it rather than staying only in this panel.
		expect(queue.has('/artist/track1')).toBe(true);
	});

	it('opens the file rather than the gate when the transfer is what failed', async () => {
		panelChrome({ response: page(HYPE) });
		chrome.downloads.download.mockRejectedValue(new Error('No room on the disk.'));
		await load();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/No room on the disk/);

		// The gate was walked; going back through it would only sign the same
		// link again.
		await click(rowButtons('open')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: signed(),
			active: true
		});
	});

	// Chrome refuses a whole download over a name it does not like, and the name
	// is the least of what was wanted: the file is asked for again without one.
	it('saves the file anyway when Chrome refuses the name, and says so', async () => {
		panelChrome({ response: page(HYPE) });
		chrome.downloads.download.mockRejectedValueOnce(new Error('Invalid filename'));
		const { downloaded } = await load();

		await click(rowButtons('get')[0]);

		expect(chrome.downloads.download).toHaveBeenNthCalledWith(2, {
			url: signed()
		});
		expect(rows()[0].state).toMatch(/saved, but Chrome renamed it/);
		// It was saved, so the track is marked and there is nothing left to do.
		expect(downloaded.has('/artist/track1')).toBe(true);
		expect(rowButtons('open')).toHaveLength(0);
	});

	it('sends a gate that wants an email to settings, not to a tab', async () => {
		panelChrome({ response: page(HYPE) });
		net({
			'hypeddit.com/886rp1': () => res(gateHtml().replace('value="sc"', 'value="email"'))
		});
		await loadInPopup();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/wants an email/);
		expect(rowButtons('open')).toHaveLength(0);

		await click(rowButtons('add email')[0]);
		expect(chrome.tabs.create).not.toHaveBeenCalled();
		expect(settingsSheet()).not.toBe(null);
		expect(field('email')).toBeTruthy();
	});

	it('leaves a link with no track behind it to be opened on its own', async () => {
		panelChrome({ tab: { id: 3, url: HYPE } });
		net({ '/gate/download/ul': () => json({ download_status: false }) });
		const { queue } = await load();

		await click(rowButtons('get')[0]);
		await click(rowButtons('open')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: HYPE,
			active: true
		});
		expect(queue.count).toBe(0);
	});

	it('runs the whole list one after another, and says what it did', async () => {
		panelChrome({ response: page(HYPE, HYPE2, LAYLO) });
		await load();

		expect(clean(allButton()!)).toBe('get all (3)');
		await click(allButton());

		expect(chrome.downloads.download).toHaveBeenCalledTimes(2);
		expect(rows().map((row) => row.state.slice(0, 30))).toEqual([
			'Hypeddit saved',
			'Hypeddit saved',
			'Laylo Laylo is a notify-me pag'
		]);
		expect(text()).toMatch(/saved 2 files, 1 to open by hand/);
		expect(clean(allButton()!)).toBe('all done');
	});

	it('reports a batch that only partly worked', async () => {
		panelChrome({ response: page(HYPE, HYPE2) });
		net({
			'hypeddit.com/artist/second-track': () => res('', { ok: false, status: 500 })
		});
		await load();

		await click(allButton());
		expect(text()).toMatch(/saved 1 of 2 — check the rows that failed/);
	});

	it('does not offer a saved row again', async () => {
		panelChrome({ response: page(HYPE) });
		await load();
		await click(rowButtons('get')[0]);
		expect(rowButtons('get')[0].disabled).toBe(true);
	});

	it('will try a failed one again', async () => {
		panelChrome({ response: page(HYPE) });
		net({ '/gate/download/ul': () => json({ download_status: false }) });
		await load();
		await click(rowButtons('get')[0]);
		expect(rowButtons('get')[0].disabled).toBe(false);
	});

	it('turns a row red when the transfer is interrupted while the panel is open', async () => {
		panelChrome({ response: page(HYPE) });
		await load();
		await click(rowButtons('get')[0]);

		chrome.testEmitDownloadChange({ id: 1, state: { current: 'interrupted' } });
		await settle();
		expect(rows()[0].state).toMatch(/the transfer was interrupted/);

		// A transfer from an earlier opening of the panel is not listed any more.
		chrome.testEmitDownloadChange({ id: 404, state: { current: 'complete' } });
		chrome.testEmitDownloadChange({ id: 1 });
		await settle();
		expect(rows()[0].state).toMatch(/the transfer was interrupted/);

		// And the row offers the file again, rather than another walk through a
		// gate that has already been through.
		await click(rowButtons('open')[0]);
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: signed(),
			active: true
		});
	});
});

// --- marking by hand ---------------------------------------------------------

// Hypeddit's Spotify step is the one a fetch cannot answer: a tab has to go
// through Spotify's own sign-in and come back. What the panel owns is the trip
// — whether one is made at all, and how many times it is worth making.
describe('a step that needs a tab', () => {
	// A trip is paced by a real timer — it is a tab being watched, not a promise
	// — so the run outlives the click that started it. Waited out by the state
	// the rows land on rather than by a guess at how long it takes.
	const walked = async () => {
		for (let round = 0; round < 60; round += 1) {
			await new Promise((resume) => setTimeout(resume, 50));
			await settle();

			if (!rows().some((row) => row.state.includes('…'))) return;
		}
	};

	const spotifyGates = () =>
		net({
			'hypeddit.com/886rp1': () => res(gateHtml('886rp1', 'sc,sp')),
			'hypeddit.com/artist/second-track': () => res(gateHtml('second', 'sc,sp'))
		});

	it('sends a tab through the step, then unlocks', async () => {
		spotifyGates();
		panelChrome({ response: page(HYPE) });
		chrome.tabs.get.mockResolvedValue({ id: 99, url: 'https://hypeddit.com/spotify_callback?c=1' });
		await load();

		await click(rowButtons('get')[0]);
		await walked();

		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: 'https://accounts.spotify.com/authorize?state=886rp1',
			active: false
		});
		// Opened behind the popup and taken away again.
		expect(chrome.tabs.remove).toHaveBeenCalledWith(99);
		expect(rows()[0].state).toMatch(/saved/);
	});

	it('asks for the one press it needs, and does not post a download without it', async () => {
		const calls = spotifyGates();
		panelChrome({ response: page(HYPE) });
		chrome.tabs.get.mockRejectedValue(new Error('No tab with id'));
		await load();

		await click(rowButtons('get')[0]);
		await walked();

		expect(rows()[0].state).toMatch(/wants a Spotify follow.*press Connect/s);
		expect(calls.some((one) => one.url.includes('/gate/download/ul'))).toBe(false);
	});

	it('asks once a run, not once a row', async () => {
		spotifyGates();
		panelChrome({ response: page(HYPE, HYPE2) });
		chrome.tabs.get.mockRejectedValue(new Error('No tab with id'));
		await load();

		await click(allButton());
		await walked();

		// The second row was told the same thing without a second tab: the answer
		// was about the account, not about the gate.
		expect(chrome.tabs.create).toHaveBeenCalledTimes(1);
		expect(rows().map((row) => row.state)).toEqual([
			expect.stringMatching(/Spotify follow/),
			expect.stringMatching(/Spotify follow/)
		]);
	});
});

describe('marking a track by hand', () => {
	it('marks the track of a row that was never walked, and unmarks it again', async () => {
		panelChrome({ response: page(HYPE) });
		const { downloaded } = await load();

		await click(markButtons()[0]);
		expect(downloaded.has('/artist/track1')).toBe(true);
		expect(markButtons()[0].getAttribute('aria-pressed')).toBe('true');
		expect(rows()[0].state).toBe('Hypeddit already marked');

		await click(markButtons()[0]);
		expect(downloaded.has('/artist/track1')).toBe(false);
		expect(markButtons()[0].getAttribute('aria-pressed')).toBe('false');
	});

	it('marks a row that only resolved to a page, which saving would never have', async () => {
		panelChrome({ response: page(LAYLO) });
		const { downloaded } = await load();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/not a gate/);
		expect(downloaded.has('/artist/track1')).toBe(false);

		await click(markButtons()[0]);
		expect(downloaded.has('/artist/track1')).toBe(true);
		// And the row keeps what it resolved to, so Open still goes there.
		expect(rowButtons('open')).toHaveLength(1);
	});

	it('marks a row that failed, for the gate finished by hand in a tab', async () => {
		panelChrome({ response: page(HYPE) });
		net({ '/gate/download/ul': () => json({ download_status: false }) });
		const { downloaded } = await load();

		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/the gate wouldn’t unlock/);
		expect(rowButtons('open')).toHaveLength(1);

		await click(markButtons()[0]);
		expect(downloaded.has('/artist/track1')).toBe(true);
	});

	it('takes off a mark that saving made, in the same click', async () => {
		panelChrome({ response: page(HYPE) });
		const { downloaded } = await load();

		await click(rowButtons('get')[0]);
		expect(downloaded.has('/artist/track1')).toBe(true);

		await click(markButtons()[0]);
		expect(downloaded.has('/artist/track1')).toBe(false);
		// Saved is still saved: the mark and the transfer are different things.
		expect(rows()[0].state).toMatch(/saved/);
	});

	it('offers no mark on a link with no track behind it', async () => {
		panelChrome({ tab: { id: 3, url: HYPE } });
		await load();
		expect(rows()).toHaveLength(1);
		expect(markButtons()).toHaveLength(0);
	});

	it('lets a marked row go when Hide downloaded is on', async () => {
		panelChrome({ response: page(HYPE, HYPE2) });
		await load();
		await click(field('hide-downloaded'));

		await click(markButtons()[0]);
		expect(rows().map((row) => row.title)).toEqual(['Artist — Track 2']);
	});
});

// --- narrowing the list ------------------------------------------------------

describe('the toggles', () => {
	const withMarks = (marked: string[]) =>
		panelChrome({
			response: page(HYPE, HYPE2),
			storage: {
				downloaded: Object.fromEntries(marked.map((p) => [p, '2026-09-08T12:00:00.000Z']))
			},
			session: {
				queue: [
					{
						permalink: '/artist/track2',
						title: 'Two',
						artist: 'A',
						details: {}
					}
				]
			}
		});

	it('says which state a row is in before it is spent', async () => {
		withMarks(['/artist/track1']);
		await load();
		expect(rows().map((row) => row.state)).toEqual(['Hypeddit already marked', 'Hypeddit in the queue']);
	});

	it('drops the marked ones, and the queued ones, and says how many are left', async () => {
		withMarks(['/artist/track1']);
		await load();

		await click(field('hide-downloaded'));
		expect(rows().map((row) => row.title)).toEqual(['Artist — Track 2']);
		expect(text()).toMatch(/1 of 2 links shown/);

		await click(field('hide-queued'));
		expect(rows()).toHaveLength(0);
		expect(text()).toMatch(/all 2 links hidden/);
	});

	it('never drops a row with no track behind it', async () => {
		panelChrome({
			tab: { id: 3, url: HYPE },
			storage: { downloaded: { '/artist/track1': 'x' } }
		});
		await load();
		await click(field('hide-downloaded'));
		await click(field('hide-queued'));
		expect(rows()).toHaveLength(1);
	});

	it('runs only what is showing, and lets a row go as saving it marks its track', async () => {
		panelChrome({ response: page(HYPE, HYPE2) });
		await load();
		await click(field('hide-downloaded'));

		await click(allButton());
		await flush();
		expect(chrome.downloads.download).toHaveBeenCalledTimes(2);
		expect(rows()).toHaveLength(0);
	});
});

describe('what the panel remembers', () => {
	// The sheet and the panel are two components, and the address is one value:
	// typed over there, used over here a moment later, without the popup having
	// closed in between. See src/lib/downloader.svelte.js for what makes that
	// one value rather than two copies of a storage key.
	it('hands a gate the address typed in settings, without being reopened', async () => {
		panelChrome({ response: page(HYPE) });

		const calls = net({
			'hypeddit.com/886rp1': () => res(gateHtml().replace('value="sc"', 'value="email"')),
			'/verifyEmailAddress': () => json({ status: 'T' })
		});

		await loadInPopup();

		await openSettings();
		await type(field('email'), 'me@example.com');
		await click(settingsButton());
		await click(rowButtons('get')[0]);

		const verify = calls.find((call) => call.url.includes('/verifyEmailAddress'));
		expect(Object.fromEntries(new URLSearchParams(verify!.body)).validateEmailAddress).toBe('me@example.com');
	});

	// The popup is destroyed by the very thing a failed row offers to do, so a
	// run that is not kept is a run that is mostly thrown away. See
	// src/tools/third-party-downloads/walked.svelte.js.
	it('gives every row back what became of it when the popup comes back', async () => {
		panelChrome({ response: page(HYPE, DROP, LAYLO) });
		await load();
		await click(allButton());
		expect(rows().map((r) => r.state)).toEqual([
			expect.stringMatching(/saved/),
			expect.stringMatching(/saved/),
			expect.stringMatching(/^Laylo Laylo is a notify-me page/)
		]);
		const before = rows();

		await reopenPanel('third-party-downloads');
		expect(rows()).toEqual(before);
		// And a row that is spent is not offered again.
		expect(rowButtons('get')[0].disabled).toBe(true);
	});

	it('does not let the gate tab a row sent you to become the whole list', async () => {
		panelChrome({ response: page(HYPE, DROP) });
		await load();
		expect(rows()).toHaveLength(2);

		// Open took the focus, so the popup closed; coming back, the tab in front
		// is the gate itself, which is a link this tool claims. The browsing
		// session is the same one, so the chrome behind it is left as it is.
		chrome.tabs.query.mockResolvedValue([{ id: 1, url: LAYLO }]);
		await reopenPanel('third-party-downloads');
		expect(rows().map((r) => r.title)).toEqual(['Artist — Track 1', 'Artist — Track 2', '9PWcf']);
		expect(meta()).toMatch(/My Playlist/);
	});

	it('leaves a walk that was still running to be run again', async () => {
		panelChrome({ response: page(HYPE) });
		// A gate that never answers: the popup closes while the row is on it.
		net({ 'hypeddit.com/886rp1': () => new Promise(() => {}) });
		await load();
		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/unlocking/);

		net();
		await reopenPanel('third-party-downloads');
		expect(rows()[0].state).toBe('Hypeddit');
		expect(rowButtons('get')[0].disabled).toBe(false);
		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/saved/);
	});

	it('gives a row its old state back when the source is switched away and back', async () => {
		panelChrome({
			response: page(HYPE),
			session: {
				queue: [
					{
						permalink: '/artist/one',
						title: 'One',
						artist: 'A',
						details: { purchaseUrl: HYPE }
					}
				]
			}
		});
		await load();
		await click(rowButtons('get')[0]);
		expect(rows()[0].state).toMatch(/saved/);

		await click(exactButton('session queue (1)'));
		expect(rows()[0].state).toMatch(/saved/);
		expect(rowButtons('get')[0].disabled).toBe(true);
	});
});
