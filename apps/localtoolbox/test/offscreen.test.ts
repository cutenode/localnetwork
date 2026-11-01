// offscreen.ts: the page the background worker walks a gate in.
//
// The walk is the real site module's, against a stubbed internet standing in
// for the gate (test/tools/gates.test.ts covers every site's walk on its own);
// what is under test is the glue around it: which links are walked at all, the
// settings the popup keeps in localStorage handed to the gate, the Spotify
// step sent back to the worker, and what the worker is told.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock } from './helpers/chrome.ts';

declare const chrome: ChromeMock;

const GATE = 'https://hypeddit.com/886rp1';

const FILE = 'https://gates.s3.amazonaws.com/886rp1/One.wav';

const AUTHORIZE = 'https://accounts.spotify.com/authorize?client_id=9c6&state=THIS-GATE';

const page = (steps: string) => `
  <html><head><meta name="csrf-token" content="CSRF"></head><body>
  <input id="current_download_file_listner" value="886rp1">
  <input id="wrndk" value="1381436x5194282">
  <input id="gvt" value="TOKEN">
  <input id="fan_gate_id" value="1381436">
  <input id="nwSteps" value="${steps}">
  <div id="step_sp"><a onclick="PopupCenterDual('${AUTHORIZE.replace(/&/g, '&amp;')}')">Connect</a></div>
  </body></html>`;

/** What the walks hand `fetch`: a method, and a form or JSON body already written out. */
type Fetch = (url: string, options?: { method?: string; body?: string }) => Promise<Response>;

/** A gate server's JSON answer, as far as these walks read one. */
type Payload = Record<string, string | boolean>;

const html = (text: string) => new Response(text);

const json = (payload: Payload) => new Response(JSON.stringify(payload));

/** Hypeddit, as far as one gate goes: its page, then the posts that walk it. Returns the posts. */
function hypeddit({ steps = 'sc', unlocks = true } = {}) {
	const posts: Record<string, Record<string, string>> = {};
	vi.stubGlobal(
		'fetch',
		vi.fn<Fetch>(async (url, options = {}) => {
			const path = new URL(url).pathname;

			if (options.method !== 'POST') return html(page(steps));
			posts[path] = Object.fromEntries(new URLSearchParams(options.body));

			if (path === '/verifyEmailAddress') return json({ status: 'T' });

			if (path === '/gate/download/ul') return json({ download_status: unlocks, URL: unlocks ? FILE : '' });

			return json({});
		})
	);

	return posts;
}

async function start() {
	globalThis.chrome = createChrome();
	vi.resetModules();
	await import('../src/offscreen.ts');
}

const walk = (url = GATE) => chrome.testSendMessage({ type: 'WALK_GATE', url });

describe('a gate whose walk ends in a file', () => {
	it('walks it to the file, with the folder it goes in and the mark setting', async () => {
		hypeddit();
		await start();

		expect(await walk()).toEqual({ ok: true, url: FILE, filename: '', subfolder: 'Hypeddit', mark: true });
	});

	it('hands the gate the address in settings, and says when marking is off', async () => {
		localStorage.setItem('pref:email', ' dj@example.com ');
		localStorage.setItem('pref:mark', '0');
		const posts = hypeddit({ steps: 'sc,email' });
		await start();

		expect(await walk()).toMatchObject({ ok: true, mark: false });
		expect(posts['/verifyEmailAddress']).toMatchObject({ validateEmailAddress: 'dj@example.com' });
	});

	it('sends the Spotify step back to the worker, which has the tabs', async () => {
		hypeddit({ steps: 'sc,sp' });
		await start();
		chrome.runtime.sendMessage.mockResolvedValue(true);

		expect(await walk()).toMatchObject({ ok: true, url: FILE });
		expect(chrome.runtime.sendMessage).toHaveBeenCalledWith({
			type: 'ROUND_TRIP',
			url: AUTHORIZE,
			landing: 'https://hypeddit.com/'
		});
	});

	it('sends the person to the gate when it will not unlock', async () => {
		hypeddit({ unlocks: false });
		await start();

		expect(await walk()).toEqual({ ok: false, openUrl: GATE });
	});

	it('sends the person where the walk ended when that is a page', async () => {
		const gate = 'https://droploud.com/gate/abc';
		const folder = 'https://www.dropbox.com/scl/fo/folder';
		vi.stubGlobal(
			'fetch',
			vi.fn<Fetch>(async (url) =>
				url.endsWith('/download')
					? json({ download_url: folder, type: 'external' })
					: url.endsWith('/step')
						? json({ ok: true })
						: html(
								`<script>self.__next_f.push([1,${JSON.stringify(
									`x:["$L33",null,{"gateData":${JSON.stringify({ track: { id: 'abc', title: 'T' }, steps: [] })}}]\n`
								)}])</script>`
							)
			)
		);
		await start();

		expect(await walk(gate)).toEqual({ ok: false, openUrl: folder });
	});
});

describe('links it leaves alone', () => {
	it('does not walk a site whose walk can end in a page or a signup', async () => {
		const fetch = vi.fn<Fetch>();
		vi.stubGlobal('fetch', fetch);
		await start();

		expect(await walk('https://laylo.com/artist/drop')).toEqual({
			ok: false,
			openUrl: 'https://laylo.com/artist/drop'
		});
		expect(await walk('https://buy.test/x')).toEqual({ ok: false, openUrl: 'https://buy.test/x' });
		expect(fetch).not.toHaveBeenCalled();
	});

	it('answers no other message', async () => {
		await start();

		expect(await chrome.testSendMessage({ type: 'SAVE_GATE', url: GATE })).toBeUndefined();
		expect(await chrome.testSendMessage(null)).toBeUndefined();
	});
});
