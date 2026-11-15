// src/tools/third-party-downloads/sites/*: the walked sites (`how: 'walk'`),
// each against a stubbed `fetch` standing in for that site: the page as HTML,
// then whatever endpoints walking it goes through. Hypeddit is the one.
//
// A walk answers the same two ways ({kind:'file'} for something to save,
// {kind:'link'} for somewhere to go) or throws with the sentence the row should
// show, so these read as: give it this page, and see which it is.
import { describe, it, expect, vi } from 'vitest';
import hypedditSite from '../../src/tools/third-party-downloads/sites/hypeddit.ts';
import { walk } from '../../src/tools/third-party-downloads/walks/index.ts';
import type { UnlockDetails } from '../../src/tools/third-party-downloads/sites/index.ts';

/** Hypeddit's walk, as the panel reaches it. */
const hypeddit = { unlock: (url: string, details: UnlockDetails) => walk(hypedditSite, url, details) };

/** What an adapter hands `fetch`, as every one of them writes it: plain headers, a string body. */
interface Sent {
	method?: string;
	credentials?: RequestCredentials;
	headers?: Record<string, string>;
	body?: string;
}

/** A request the stubbed `fetch` was handed. */
interface Call {
	url: string;
	options: Sent;
	body: string;
}

/** One stubbed endpoint: what it answers the request with. */
type Route = (options: Sent) => Answer;

/** A response as the stubbed `fetch` gives one: the fields the adapters read. */
type Answer = ReturnType<typeof res>;

/** What a stubbed endpoint answers with: a page's HTML, or a JSON payload (which drops undefined keys). */
type Json = string | number | boolean | null | Json[] | { [key: string]: Json | undefined };

/** The settings a gate is handed: none, as a fresh install has, unless a test gives some. */
const details = (over: Partial<UnlockDetails> = {}): UnlockDetails => ({ email: '', name: '', ...over });

type Connect = NonNullable<UnlockDetails['connect']>;

// --- a stubbed internet ------------------------------------------------------

const res = (body: Json, { ok = true, status = 200, url = '', redirected = false } = {}) => {
	// A page is its HTML as it stands; a payload goes over the wire as JSON.
	const text = body instanceof Object ? JSON.stringify(body) : String(body);

	return {
		ok,
		status,
		url,
		redirected,
		body: { cancel: vi.fn<() => void>() },
		text: async () => text,
		json: async () => JSON.parse(text)
	};
};

/**
 * Installs a `fetch` that answers the first route whose key `url` contains.
 * Routes are functions so a test can make one fail. Returns the calls made.
 */
function net(routes: Record<string, Route>) {
	const calls: Call[] = [];
	vi.stubGlobal(
		'fetch',
		vi.fn<(url: string, options?: Sent) => Promise<Answer>>(async (url, options = {}) => {
			calls.push({ url, options, body: options.body ?? '' });
			const key = Object.keys(routes).find((match) => url.includes(match));

			if (!key) throw new Error(`nothing stubbed for ${url}`);

			return routes[key](options);
		})
	);

	return calls;
}

const form = (calls: Call[], match: string) => {
	const call = calls.find((one) => one.url.includes(match) && one.options.method === 'POST');

	return call ? Object.fromEntries(new URLSearchParams(call.body)) : null;
};

const message = async (promise: Promise<unknown>) => {
	try {
		await promise;

		return null;
	} catch (error) {
		return error instanceof Error ? error.message : String(error);
	}
};

// --- Hypeddit ----------------------------------------------------------------

describe('Hypeddit', () => {
	const GATE = 'https://hypeddit.com/886rp1';

	// The Spotify step, as a gate carries it: a Connect button whose onclick
	// wraps Spotify's authorize URL in the page's own popup opener, and a
	// second one further down the page that connects an account rather than
	// answers this gate.
	const AUTHORIZE =
		'https://accounts.spotify.com/authorize?client_id=9c6&' +
		'redirect_uri=https%3A%2F%2Fhypeddit.com%2Fspotify_callback&state=THIS-GATE';

	const spotifyStep = `
    <div class="hype-sidebar-step-form" id="step_sp">
      <a href="javascript:void(0)" onclick="inAppSetup(this,2);PopupCenterDual('${AUTHORIZE.replace(
				/&/g,
				'&amp;'
			)}','Spotify',700,700)">Connect</a>
    </div>
    <ul class="nav"><li><a onclick="PopupCenterDual('https://accounts.spotify.com/authorize?client_id=9c6&amp;state=AN-ACCOUNT')">Connect Spotify</a></li></ul>`;

	// The title as Hypeddit writes it: "<track> by <artist> | Free Download on Hypeddit".
	const TITLE = 'FEIN (KAKU Remix) by KAKU, D4VIS | Free Download on Hypeddit';

	const page = ({ steps = 'sc', csrf = 'CSRF', uid = '886rp1', duration = '236000', title = TITLE } = {}) => `
    <html><head><title>${title}</title><meta name="csrf-token" content="${csrf}"></head><body>
    <input id="current_download_file_listner" value="${uid}">
    <input name="wrndk" id="wrndk"  value="1381436x5194282"/>
    <input id="gvt" value="TOKEN">
    <input id="fan_gate_id" value='1381436'>
    <input id="nwSteps" value="${steps}">
    <input id="is_skippable" value="0">
    <input id="duration" value="${duration}">
    ${steps.split(',').includes('sp') ? spotifyStep : ''}
    </body></html>`;

	const signed = (name = 'One.wav') =>
		`https://gates.s3.amazonaws.com/886rp1?response-content-disposition=${encodeURIComponent(
			`attachment; filename="${name}"`
		)}`;

	const routes = (over = {}) => ({
		'/gate/ge': () => res({ ok: 1 }),
		'/verifyEmailAddress': () => res({ attempts: 'no', status: 'T' }),
		'/gate/download/ul': () => res({ download_status: true, URL: signed() }),
		'hypeddit.com/886rp1': () => res(page()),
		...over
	});

	it('reads the page’s hidden fields and posts them back', async () => {
		const calls = net(routes());
		expect(await hypeddit.unlock(GATE, details())).toEqual({
			url: signed()
		});

		// The visit is registered before the download is asked for: gates that
		// count visits refuse a download from a session that never paid one.
		expect(calls.map((one) => new URL(one.url).pathname)).toEqual(['/886rp1', '/gate/ge', '/gate/download/ul']);

		// Every request carries the session cookie the page set, and every post
		// the page's nonce with it: without both the post is a stranger's.
		for (const one of calls) expect(one.options.credentials).toBe('include');

		for (const one of calls.slice(1)) expect(one.options.headers!['X-CSRF-TOKEN']).toBe('CSRF');

		expect(form(calls, '/gate/ge')).toEqual({ vt: 'TOKEN', uid: '886rp1' });
		const post = form(calls, '/gate/download/ul');
		expect(post).toMatchObject({
			file: '886rp1',
			wrndk: '1381436x5194282',
			steps: 'sc',
			download_action: 'DOWNLOAD',
			email: '',
			// The page's own form, as its Download button posts it.
			download_visit: 'true',
			profile_downloads: 'true',
			page: 'nonsingle',
			gvf: '0'
		});
		expect(Number(post!.time)).toBeLessThan(236000);
	});

	it('walks a gate that gives neither a duration nor a step list', async () => {
		const calls = net(
			routes({
				'hypeddit.com/886rp1': () => res(page({ duration: '', steps: '' }))
			})
		);

		await hypeddit.unlock(GATE, details());
		expect(form(calls, '/gate/download/ul')).toMatchObject({
			steps: '',
			email: ''
		});
	});

	it('hands a real address to the gates that ask for one, and only those', async () => {
		const calls = net(routes({ 'hypeddit.com/886rp1': () => res(page({ steps: 'sc,email' })) }));
		await hypeddit.unlock(GATE, { email: 'me@example.com', name: 'Me' });
		expect(form(calls, '/verifyEmailAddress')).toMatchObject({
			validateEmailAddress: 'me@example.com',
			email_name: 'Me',
			fan_gate_id: '1381436'
		});
		expect(form(calls, '/gate/download/ul')!.email).toBe('me@example.com');
		expect(calls.map((one) => new URL(one.url).pathname)).toEqual([
			'/886rp1',
			'/gate/ge',
			'/verifyEmailAddress',
			'/gate/download/ul'
		]);
	});

	it('asks for one rather than inventing it', async () => {
		net(routes({ 'hypeddit.com/886rp1': () => res(page({ steps: 'email' })) }));
		expect(await message(hypeddit.unlock(GATE, details({ email: '' })))).toMatch(/wants an email/);
	});

	it('says what an address Hypeddit will not take means', async () => {
		net(
			routes({
				'hypeddit.com/886rp1': () => res(page({ steps: 'email' })),
				'/verifyEmailAddress': () => res({ status: 'F' })
			})
		);
		const error = await hypeddit.unlock(GATE, details({ email: 'no@example.com' })).catch((e) => e);
		expect(error.message).toMatch(/real, deliverable/);
		// An address is the settings sheet's to fix; a trip to the gate would not.
		expect(error.next).toBe('email');
	});

	it('says a CAPTCHA has to be answered in a tab', async () => {
		net(
			routes({
				'hypeddit.com/886rp1': () => res(page({ steps: 'email' })),
				'/verifyEmailAddress': () => res({ attempts: 'over' })
			})
		);
		expect(await message(hypeddit.unlock(GATE, details({ email: 'me@example.com' })))).toMatch(/CAPTCHA/);
	});

	it('reports a page that is not a gate, one that will not load, and one that will not open', async () => {
		net(routes({ 'hypeddit.com/886rp1': () => res('<html></html>') }));
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/not a Hypeddit gate/);

		net(
			routes({
				'hypeddit.com/886rp1': () => res('', { ok: false, status: 404 })
			})
		);
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/HTTP 404/);

		net(routes({ '/gate/download/ul': () => res({ download_status: false }) }));
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/wouldn’t unlock/);

		net(routes({ '/gate/ge': () => res('', { ok: false, status: 500 }) }));
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/answered 500/);
	});

	// Hypeddit does the Spotify following itself, off a token of its own, so
	// the step is not something the post can claim: it is a trip through
	// Spotify's own sign-in, and the panel is what can make one.
	describe('a gate with a Spotify step', () => {
		const spotify = (over = {}) =>
			routes({
				'hypeddit.com/886rp1': () => res(page({ steps: 'sc,sp' })),
				...over
			});

		it('sends a tab through the step’s own connect, not the page’s other ones', async () => {
			net(spotify());
			const connect = vi.fn<Connect>(async () => true);
			expect(await hypeddit.unlock(GATE, details({ connect }))).toEqual({
				url: signed(),
				// The follow happened on the person's own Spotify, so the row names whose.
				note: 'followed KAKU, D4VIS on Spotify, saved the track'
			});

			expect(connect).toHaveBeenCalledWith(AUTHORIZE, {
				landing: 'https://hypeddit.com/'
			});
		});

		it('says "the artist" when the page title does not name one', async () => {
			for (const title of ['FEIN | Free Download on Hypeddit', 'FEIN by KAKU', '']) {
				net(spotify({ 'hypeddit.com/886rp1': () => res(page({ steps: 'sc,sp', title })) }));
				const answer = await hypeddit.unlock(GATE, details({ connect: async () => true }));

				expect('note' in answer && answer.note).toBe('followed the artist on Spotify, saved the track');
			}
		});

		it('goes through it before the download post, not after a refusal', async () => {
			const order: string[] = [];
			net(
				spotify({
					'/gate/download/ul': () => {
						order.push('post');

						return res({ download_status: true, URL: signed() });
					}
				})
			);

			const connect = vi.fn<Connect>(async () => {
				order.push('connect');

				return true;
			});

			await hypeddit.unlock(GATE, details({ connect }));
			expect(order).toEqual(['connect', 'post']);
		});

		it('takes a step section it cannot find a connect in as none', async () => {
			net(
				spotify({
					'hypeddit.com/886rp1': () =>
						res(page({ steps: 'sc,sp' }).replace(/onclick="inAppSetup[^"]*"/, 'onclick="gateNextSlide(this)"'))
				})
			);
			const connect = vi.fn<Connect>(async () => true);
			expect(await message(hypeddit.unlock(GATE, details({ connect })))).toMatch(/Spotify follow/);
			expect(connect).not.toHaveBeenCalled();
		});

		it('takes a gate whose step section is missing as having no connect, whatever else the page links', async () => {
			// The nav's account Connect is still on the page; it is not this gate's.
			net(
				spotify({
					'hypeddit.com/886rp1': () => res(page({ steps: 'sc,sp' }).replace('id="step_sp"', 'id="step_other"'))
				})
			);
			expect(await message(hypeddit.unlock(GATE, details({ connect: async () => true })))).toMatch(/Spotify follow/);
		});

		it('leaves the gates without one alone', async () => {
			net(routes());
			const connect = vi.fn<Connect>(async () => true);
			await hypeddit.unlock(GATE, details({ connect }));
			expect(connect).not.toHaveBeenCalled();
		});

		it('asks for the one press it needs when the trip wants a person', async () => {
			net(spotify());
			expect(await message(hypeddit.unlock(GATE, details({ connect: async () => false })))).toMatch(
				/Spotify follow.*press Connect/s
			);
		});

		it('says one press is enough for every gate after it', async () => {
			net(spotify());
			expect(await message(hypeddit.unlock(GATE, details({ connect: async () => false })))).toMatch(
				/press Connect once, and the rest of these unlock here\.$/
			);
		});

		it('says the same where no tab can be opened at all', async () => {
			net(spotify());
			expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/Spotify follow/);
		});

		it('does not post a download it knows will be refused', async () => {
			const calls = net(spotify());
			await message(hypeddit.unlock(GATE, details({ connect: async () => false })));
			expect(calls.some((one) => one.url.includes('/gate/download/ul'))).toBe(false);
		});
	});

	it('names the steps a refusal was probably about', async () => {
		net(
			routes({
				'hypeddit.com/886rp1': () => res(page({ steps: 'sc,dz' })),
				'/gate/download/ul': () => res({ download_status: false, URL: '' })
			})
		);
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/wants Deezer/);
	});

	it('takes the fields however the template quotes them', async () => {
		const calls = net(routes({ 'hypeddit.com/886rp1': () => res(page({ csrf: 'x', uid: 'a&amp;b' })) }));
		await hypeddit.unlock(GATE, details());
		const download = calls.find((one) => one.url.includes('/gate/download/ul'));
		expect(form(calls, '/gate/download/ul')!.file).toBe('a&b');
		expect(download!.options.headers!['X-CSRF-TOKEN']).toBe('x');
	});

	it('decodes every entity a template escapes a field with', async () => {
		const calls = net(
			routes({ 'hypeddit.com/886rp1': () => res(page({ uid: 'a&amp;b&quot;c&#39;d&#039;e&lt;f&gt;g' })) })
		);

		await hypeddit.unlock(GATE, details());
		expect(form(calls, '/gate/download/ul')!.file).toBe(`a&b"c'd'e<f>g`);
	});

	it('finds a field in a tag written in capitals', async () => {
		const calls = net(
			routes({
				'hypeddit.com/886rp1': () =>
					res(page().replace('<input id="current_download_file_listner"', '<INPUT ID="current_download_file_listner"'))
			})
		);

		await hypeddit.unlock(GATE, details());
		expect(form(calls, '/gate/download/ul')!.file).toBe('886rp1');
	});

	it('finds the token among other attributes, and sends an empty one from a page without it', async () => {
		const token = (html: string) => {
			const calls = net(routes({ 'hypeddit.com/886rp1': () => res(html) }));

			return hypeddit
				.unlock(GATE, details())
				.then(() => calls.find((one) => one.url.includes('/gate/download/ul'))!.options.headers!['X-CSRF-TOKEN']);
		};

		expect(
			await token(page().replace('<meta name="csrf-token"', '<meta data-turbo="false" name="csrf-token" data-keep="1"'))
		).toBe('CSRF');
		expect(await token(page().replace(/<meta[^>]*>/, ''))).toBe('');
	});

	it('takes a page without the nonce as no gate', async () => {
		net(routes({ 'hypeddit.com/886rp1': () => res(page().replace(/<input name="wrndk"[^>]*>/, '')) }));
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/not a Hypeddit gate/);
	});

	it('asks for the page as a page, and posts the way the page’s own script does', async () => {
		const calls = net(routes({ 'hypeddit.com/886rp1': () => res(page({ steps: 'email' })) }));
		await hypeddit.unlock(GATE, details({ email: 'me@example.com', name: 'Me' }));

		expect(calls[0].options.headers).toEqual({ Accept: 'text/html' });

		// A form-encoded XHR is what the endpoints parse and answer in JSON; a
		// string body sent without a type would arrive as plain text.
		for (const one of calls.slice(1)) {
			expect(one.options.headers).toEqual({
				'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8',
				'X-Requested-With': 'XMLHttpRequest',
				'X-CSRF-TOKEN': 'CSRF',
				Accept: 'application/json, text/javascript, */*; q=0.01'
			});
		}

		// Every field the page's own form has, the blank ones included.
		expect(form(calls, '/verifyEmailAddress')).toEqual({
			validateEmailAddress: 'me@example.com',
			fan_gate_id: '1381436',
			email_name: 'Me',
			adcode: '',
			hypesource: ''
		});
		expect(form(calls, '/gate/download/ul')).toEqual({
			file: '886rp1',
			download_visit: 'true',
			profile_downloads: 'true',
			time: expect.any(String),
			sc_comment_text: '',
			yt_comment_text: '',
			page: 'nonsingle',
			is_skippable: '0',
			steps: 'email',
			email: 'me@example.com',
			download_action: 'DOWNLOAD',
			wrndk: '1381436x5194282',
			is_mobile: '',
			external_id: '',
			hypesource: '',
			adcode: '',
			gvf: '0'
		});
	});

	it('stamps the post at a point inside the track, or inside three minutes of one with no length', async () => {
		vi.spyOn(Math, 'random').mockReturnValue(0.5);

		const time = async (duration: string) => {
			const calls = net(routes({ 'hypeddit.com/886rp1': () => res(page({ duration })) }));
			await hypeddit.unlock(GATE, details());

			return form(calls, '/gate/download/ul')!.time;
		};

		expect(await time('1000')).toBe('500');
		expect(await time('')).toBe('90000');
		expect(await time('soon')).toBe('90000');
	});

	it('takes an answer with no file in it as a gate that stayed shut', async () => {
		net(routes({ '/gate/download/ul': () => res({ download_status: true, URL: '' }) }));
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/wouldn’t unlock/);

		net(routes({ '/gate/download/ul': () => res({ download_status: false, URL: signed() }) }));
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/wouldn’t unlock/);
	});

	it('names every step a refusal was probably about', async () => {
		net(
			routes({
				'hypeddit.com/886rp1': () => res(page({ steps: 'dz,sc,ap' })),
				'/gate/download/ul': () => res({ download_status: false, URL: '' })
			})
		);
		expect(await message(hypeddit.unlock(GATE, details()))).toMatch(/wants Deezer and Apple Music\./);
	});
});

// --- Droploud ----------------------------------------------------------------

describe('every walk', () => {
	it('throws rather than answering when the site cannot be reached at all', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn<typeof fetch>(async () => {
				throw new TypeError('Failed to fetch');
			})
		);

		await expect(hypeddit.unlock('https://hypeddit.com/abc', details())).rejects.toThrow(Error);
	});
});
