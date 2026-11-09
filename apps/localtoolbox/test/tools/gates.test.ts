// src/tools/third-party-downloads/sites/*: one adapter per site, each against
// a stubbed `fetch` standing in for that site: the page as HTML, then whatever
// endpoints walking it goes through.
//
// Every adapter answers the same two ways ({kind:'file'} for something to save,
// {kind:'link'} for somewhere to go) or throws with the sentence the row should
// show, so these read as: give it this page, and see which it is.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import droploud from '../../src/tools/third-party-downloads/sites/droploud.ts';
import featurefm from '../../src/tools/third-party-downloads/sites/featurefm.ts';
import hypeddit from '../../src/tools/third-party-downloads/sites/hypeddit.ts';
import type { UnlockDetails, Unlocked } from '../../src/tools/third-party-downloads/sites/index.ts';
import laylo from '../../src/tools/third-party-downloads/sites/laylo.ts';
import pumpyoursound from '../../src/tools/third-party-downloads/sites/pumpyoursound.ts';
import { fanLinkTitle, fanLinkUrl, fanlink, toneden } from '../../src/tools/third-party-downloads/sites/toneden.ts';

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

/** Why an answer is somewhere to go rather than a file, or undefined for a file. */
const why = (answer: Unlocked) => ('why' in answer ? answer.why : undefined);

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

const sent = (calls: Call[], match: string) => {
	const call = calls.find((one) => one.url.includes(match));

	return call ? JSON.parse(call.body) : null;
};

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

	const page = ({ steps = 'sc', csrf = 'CSRF', uid = '886rp1', duration = '236000' } = {}) => `
    <html><head><meta name="csrf-token" content="${csrf}"></head><body>
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
			kind: 'file',
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
				kind: 'file',
				url: signed()
			});

			expect(connect).toHaveBeenCalledWith(AUTHORIZE, {
				landing: 'https://hypeddit.com/'
			});
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

describe('Droploud', () => {
	const GATE = 'https://droploud.com/gate/abc';
	const FILE = 'https://cdn.droploud.test/tracks/Front%20to%20Back.wav';

	const STEPS = [
		{ id: 'sc_follow', type: 'soundcloud_follow' },
		{ id: 'sc_like', type: 'soundcloud_like' },
		{ id: 'sc_repost', type: 'soundcloud_repost' }
	];

	const page = (gate: Json) =>
		`<script>self.__next_f.push([1,${JSON.stringify(
			`x:["$L33",null,{"gateData":${JSON.stringify(gate)}}]\n`
		)}])</script>`;

	const gate = (over = {}) => ({
		track: { id: 'abc', title: 'Front to Back' },
		artist: { display_name: 'D!lemma' },
		steps: STEPS,
		...over
	});

	const routes = (over = {}) => ({
		'/soundcloud/verify-repost': () => res({ gave_up: true }),
		'/gate/abc/step': () => res({ ok: true }),
		'/gate/abc/download': () => res({ download_url: FILE, type: 'native' }),
		'droploud.com/gate/abc': () => res(page(gate())),
		...over
	});

	beforeEach(() => {
		// @ts-expect-error -- a session id that reads as one, not a UUID
		vi.spyOn(crypto, 'randomUUID').mockReturnValue('SESSION');
	});

	it('reports each step the page would, then asks for the file', async () => {
		const calls = net(routes());
		expect(await droploud.unlock(GATE, details())).toEqual({
			kind: 'file',
			url: FILE,
			title: 'D!lemma — Front to Back'
		});

		expect(calls.filter((one) => one.url.endsWith('/step')).map((one) => JSON.parse(one.body))).toEqual([
			{ session_id: 'SESSION', step: 'sc_follow' },
			{ session_id: 'SESSION', step: 'sc_like' }
		]);
		expect(sent(calls, '/gate/abc/download')).toEqual({
			session_id: 'SESSION'
		});
		// The download is asked for last: it is the call that checks the steps.
		expect(calls.map((one) => new URL(one.url).pathname.replace(/^\/api/, ''))).toEqual([
			'/gate/abc',
			'/gate/abc/step',
			'/gate/abc/step',
			'/soundcloud/verify-repost',
			'/gate/abc/download'
		]);

		for (const one of calls) expect(one.options.credentials).toBe('include');
	});

	it('reports a step with a numeric id, and skips only a step it cannot read', async () => {
		const steps = [{ id: 7, type: 'soundcloud_follow' }, null, { type: 'soundcloud_like' }, STEPS[2]];
		const calls = net(routes({ 'droploud.com/gate/abc': () => res(page(gate({ steps }))) }));

		await droploud.unlock(GATE, details());

		expect(calls.filter((one) => one.url.endsWith('/step')).map((one) => JSON.parse(one.body))).toEqual([
			{ session_id: 'SESSION', step: 7 }
		]);
		// The repost step after the unreadable ones is still given up.
		expect(sent(calls, '/soundcloud/verify-repost')).toEqual({ track_id: 'abc', session_id: 'SESSION', give_up: true });
	});

	it('gives the repost step up rather than claiming it', async () => {
		const calls = net(routes());
		await droploud.unlock(GATE, details());
		expect(sent(calls, '/soundcloud/verify-repost')).toEqual({
			track_id: 'abc',
			session_id: 'SESSION',
			give_up: true
		});
	});

	it('says so when a gate will not open without the repost really being done', async () => {
		net(
			routes({
				'/soundcloud/verify-repost': () => res({ gave_up: false }),
				'/gate/abc/download': () => res({ error: 'Gate not completed' }, { ok: false, status: 403 })
			})
		);
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/enforcing the repost step/);
	});

	it('takes a repost Droploud says it found', async () => {
		// Found rather than given up: so a refusal after it is not about the
		// repost, and the row passes on what Droploud said instead.
		net(
			routes({
				'/soundcloud/verify-repost': () => res({ found: true }),
				'/gate/abc/download': () => res({ error: 'Gate not completed' }, { ok: false, status: 403 })
			})
		);
		const said = await message(droploud.unlock(GATE, details()));
		expect(said).toMatch(/"Gate not completed" \(HTTP 403\)/);
		expect(said).not.toMatch(/enforcing the repost/);
	});

	it('does not blame a repost step a gate never had', async () => {
		net(
			routes({
				'droploud.com/gate/abc': () => res(page(gate({ steps: STEPS.slice(0, 2) }))),
				'/gate/abc/download': () => res({ error: 'Gate not completed' }, { ok: false, status: 403 })
			})
		);
		const said = await message(droploud.unlock(GATE, details()));
		expect(said).toMatch(/"Gate not completed" \(HTTP 403\)/);
		expect(said).not.toMatch(/repost/);
	});

	it('passes Droploud’s own complaint on, and copes with one it cannot read', async () => {
		net(
			routes({
				'/gate/abc/download': () => res({ error: 'Gate not completed' }, { ok: false, status: 403 })
			})
		);
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/"Gate not completed" \(HTTP 403\)/);

		net(
			routes({
				'/gate/abc/download': () => res('<h1>nope</h1>', { ok: false, status: 502 })
			})
		);
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/answered 502/);
	});

	it('offers a gate the artist pointed at a page as somewhere to open', async () => {
		net(
			routes({
				'/gate/abc/download': () =>
					res({
						download_url: 'https://patreon.com/posts/1',
						type: 'external'
					})
			})
		);
		expect(await droploud.unlock(GATE, details())).toMatchObject({
			kind: 'link',
			url: 'https://patreon.com/posts/1',
			why: expect.stringMatching(/page rather than a file/)
		});
	});

	it('reports a page it cannot read as a gate', async () => {
		net(routes({ 'droploud.com/gate/abc': () => res('<html></html>') }));
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/not a Droploud gate/);

		net(routes({ 'droploud.com/gate/abc': () => res(page({ track: {} })) }));
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/not a Droploud gate/);

		net(
			routes({
				'droploud.com/gate/abc': () => res('', { ok: false, status: 404 })
			})
		);
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/HTTP 404/);
	});

	it('walks a gate that names neither artist nor steps', async () => {
		net(
			routes({
				'droploud.com/gate/abc': () => res(page({ track: { id: 'abc' }, artist: null, steps: null }))
			})
		);
		expect(await droploud.unlock(GATE, details())).toEqual({
			kind: 'file',
			url: FILE,
			title: ''
		});
	});

	it('asks for the page as a page, and posts JSON to the API', async () => {
		const calls = net(routes());
		await droploud.unlock(GATE, details());

		expect(calls[0].options.headers).toEqual({ Accept: 'text/html' });

		for (const one of calls.slice(1)) {
			expect(one.options.method).toBe('POST');
			expect(one.options.headers).toEqual({ 'Content-Type': 'application/json', Accept: 'application/json' });
		}
	});

	it('says where the repost step has to be done', async () => {
		net(
			routes({
				'/soundcloud/verify-repost': () => res({ gave_up: false }),
				'/gate/abc/download': () => res({ error: 'Gate not completed' }, { ok: false, status: 403 })
			})
		);
		expect(await message(droploud.unlock(GATE, details()))).toMatch(
			/connected on the page itself\. open it in a tab\.$/
		);
	});

	it('takes a download answer with no URL in it as a refusal', async () => {
		net(routes({ '/gate/abc/download': () => res({ type: 'native' }) }));
		expect(await message(droploud.unlock(GATE, details()))).toMatch(/answered 200 to the download/);
	});
});

// --- ToneDen -----------------------------------------------------------------

describe('ToneDen', () => {
	const POST = 'https://www.toneden.io/after-wes/post/never-felt';
	const FILE = 'https://files.toneden.test/neverfelt.wav';

	const post = (over = {}) => ({
		id: 6201383,
		title: 'never felt',
		unlock_type: 'download',
		download_source: 'external',
		download_display_url: 'neverfelt.wav',
		direct_url: '',
		author: { display_name: 'After Wes' },
		...over
	});

	const routes = (over = {}) => ({
		'/api/v1/attachments/6201383/download': () => res({ url: FILE }),
		'/api/v1/posts?': () => res({ post: post() }),
		...over
	});

	it('looks the post up without an account, then asks for the file with the session there is', async () => {
		const calls = net(routes());
		expect(await toneden.unlock(POST, details())).toEqual({
			kind: 'file',
			url: FILE,
			title: 'After Wes — never felt',
			filename: 'neverfelt.wav'
		});
		expect(calls[0].url).toBe('https://www.toneden.io/api/v1/posts?author_username=after-wes&link_path=never-felt');
		expect(calls[1].options.credentials).toBe('include');
	});

	it('says what to do about a gate that has not been through in this browser', async () => {
		net(
			routes({
				'/api/v1/attachments/6201383/download': () => res({ message: 'Please login' }, { ok: false, status: 401 })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(/connect the account once/);
	});

	it('finishes a redirect gate itself, because there is nothing to unlock', async () => {
		net(
			routes({
				'/api/v1/posts?': () =>
					res({
						post: post({
							download_source: 'redirect',
							download_display_url: 'https://www.dropbox.com/s/x/One.wav?dl=1'
						})
					})
			})
		);
		expect(await toneden.unlock(POST, details())).toEqual({
			kind: 'file',
			url: 'https://www.dropbox.com/s/x/One.wav?dl=1',
			title: 'After Wes — never felt'
		});
	});

	it('offers a redirect gate that points at a shop as somewhere to open', async () => {
		net(
			routes({
				'/api/v1/posts?': () =>
					res({
						post: post({
							download_source: 'redirect',
							direct_url: 'https://beatport.com/x'
						})
					})
			})
		);
		expect(await toneden.unlock(POST, details())).toMatchObject({
			kind: 'link',
			url: 'https://beatport.com/x',
			why: expect.stringMatching(/redirect/)
		});
	});

	it('falls through to the download when a redirect gate names nowhere to go', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ download_source: 'redirect' }) })
			})
		);
		expect((await toneden.unlock(POST, details())).kind).toBe('file');
	});

	it('takes the file URL from wherever ToneDen puts it', async () => {
		for (const body of [{ URL: FILE }, { download_url: FILE }, { downloadUrl: FILE }]) {
			net(routes({ '/api/v1/attachments/6201383/download': () => res(body) }));
			expect((await toneden.unlock(POST, details())).url).toBe(FILE);
		}

		net(
			routes({
				'/api/v1/attachments/6201383/download': () => res(JSON.stringify(FILE))
			})
		);
		expect((await toneden.unlock(POST, details())).url).toBe(FILE);

		// A redirect straight at the file: the body is the file, not JSON.
		net(
			routes({
				'/api/v1/attachments/6201383/download': () => res('<binary>', { url: FILE, redirected: true })
			})
		);
		expect((await toneden.unlock(POST, details())).url).toBe(FILE);
	});

	it('says so when the answer carries no file at all', async () => {
		net(routes({ '/api/v1/attachments/6201383/download': () => res({}) }));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/connect the account/);
	});

	it('offers a download that came back as a page, rather than calling it a file', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ download_display_url: '' }) }),
				'/api/v1/attachments/6201383/download': () => res({ url: 'https://toneden.io/thanks' })
			})
		);
		expect(await toneden.unlock(POST, details())).toMatchObject({
			kind: 'link',
			url: 'https://toneden.io/thanks',
			why: 'ToneDen answered with a page rather than a file.'
		});
	});

	it('says a gate that is not a download has no file behind it', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ unlock_type: 'stream' }) })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(/"stream" gate/);
	});

	it('names a post that has neither an artist nor a title after its link', async () => {
		net(
			routes({
				'/api/v1/posts?': () =>
					res({
						post: {
							id: 6201383,
							download_source: 'external',
							download_display_url: ''
						}
					})
			})
		);
		expect(await toneden.unlock(POST, details())).toEqual({
			kind: 'file',
			url: FILE,
			title: '',
			filename: ''
		});
	});

	it('falls through when a redirect gate names nothing at all', async () => {
		net(
			routes({
				'/api/v1/posts?': () =>
					res({
						post: post({
							download_source: 'redirect',
							download_display_url: ''
						})
					})
			})
		);
		expect((await toneden.unlock(POST, details())).kind).toBe('file');
	});

	it('names the account a closed gate wants, rather than the status it answered with', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ platform: 'twitch' }) }),
				'/api/v1/attachments/6201383/download': () =>
					res({ error: true, message: 'Bad request' }, { ok: false, status: 400 }),
				'/gateMetadata': () =>
					res({
						metadata: { id: 6201383, is_gate_open: false },
						viewer_id: 42
					})
			})
		);
		const said = await message(toneden.unlock(POST, details()));
		expect(said).toMatch(/wants Twitch/);
		expect(said).toMatch(/signed in to hasn’t been through this gate/);
	});

	it('takes a viewer id given as a string for someone, and an empty one for no one', async () => {
		const closed = (viewer: Json) =>
			routes({
				'/api/v1/posts?': () => res({ post: post({ platform: 'twitch' }) }),
				'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: viewer })
			});

		net(closed('42'));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/signed in to hasn’t been through this gate/);

		net(closed(''));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/ToneDen doesn’t know this browser/);
	});

	it('says the browser is a stranger when ToneDen knows no one at all', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ platform: 'soundcloud' }) }),
				'/api/v1/attachments/6201383/download': () =>
					res({ error: true, message: 'Bad request' }, { ok: false, status: 400 }),
				'/gateMetadata': () =>
					res({
						metadata: { id: 6201383, is_gate_open: false },
						viewer_id: null
					})
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(
			// Any character stands in for the dash.
			/^this gate wants SoundCloud, and ToneDen doesn’t know this browser\. open the link, connect once, then run the row again . the session sticks, and later gates by the same route go straight through\.$/
		);
	});

	it('lists every account a multi-action gate wants, and how many of them count', async () => {
		const both = (over = {}) =>
			routes({
				'/api/v1/posts?': () =>
					res({
						post: post({
							platform: 'spotify',
							multi_action_enabled: true,
							platforms: ['spotify', 'youtube'],
							...over
						})
					}),
				'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: 42 })
			});

		net(both({}));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/Spotify and YouTube/);

		net(both({ is_or_condition: true }));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/Spotify or YouTube/);

		// A platform that is not a name is passed over, not the list it is in.
		net(both({ platforms: ['spotify', 7, 'youtube'] }));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/Spotify and YouTube/);
	});

	it('sends an email gate to the page rather than naming an account to connect', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ platform: 'email' }) }),
				'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: null })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toBe(
			'this gate only wants an email address, given on ToneDen’s own page. open the link, leave one, then run the row again.'
		);
	});

	it('keeps ToneDen’s own words for a gate that is open and still says no', async () => {
		net(
			routes({
				'/api/v1/attachments/6201383/download': () =>
					res({ error: true, message: 'This download has expired.' }, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: true }, viewer_id: 42 })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toBe('ToneDen said: This download has expired.');
	});

	it('names no account at all for a closed gate that does not say what it wants', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ platform: null }) }),
				'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: 42 })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(/wants an account of its own/);
	});

	it('keeps ToneDen’s own words when the gate lookup says nothing about the gate', async () => {
		net(
			routes({
				'/api/v1/attachments/6201383/download': () =>
					res({ error: true, message: 'Bad request' }, { ok: false, status: 400 }),
				// An answer with no `metadata` on it is no answer to the question.
				'/gateMetadata': () => res({ viewer_id: 42 })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toBe('ToneDen said: Bad request');
	});

	it('says a gate with the file switched off has nothing behind it', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ skip_download: true }) })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(/no file behind it/);
	});

	it('reports a link, and a lookup, that go nowhere', async () => {
		net(routes());
		expect(await message(toneden.unlock('https://toneden.io/artist', details()))).toMatch(/not a ToneDen post link/);

		net(routes({ '/api/v1/posts?': () => res({}) }));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/doesn’t know that post/);

		net(routes({ '/api/v1/posts?': () => res({}, { ok: false, status: 500 }) }));
		expect(await message(toneden.unlock(POST, details()))).toMatch(/answered 500/);
	});

	it('takes only <artist>/post/<slug> for a post link', async () => {
		for (const url of ['https://toneden.io/after-wes/post', 'https://toneden.io/after-wes/tracks/never-felt']) {
			net(routes());
			expect(await message(toneden.unlock(url, details()))).toMatch(/not a ToneDen post link/);
		}
	});

	it('looks up a post whose slug runs over more than one segment', async () => {
		// Only the full slug is stubbed: a lookup for any other path finds nothing.
		net({
			'/api/v1/attachments/6201383/download': () => res({ url: FILE }),
			'link_path=never-felt%2Fextended': () => res({ post: post() })
		});
		expect((await toneden.unlock(`${POST}/extended`, details())).url).toBe(FILE);
	});

	it('asks the API for JSON, with the browser’s session', async () => {
		const calls = net(routes());
		await toneden.unlock(POST, details());
		expect(calls.map((call) => call.options)).toEqual([
			{ credentials: 'include', headers: { Accept: 'application/json' } },
			{ credentials: 'include', headers: { Accept: 'application/json' } }
		]);
	});

	it('quotes ToneDen without the whitespace around its sentence', async () => {
		net(
			routes({
				'/api/v1/attachments/6201383/download': () =>
					res({ message: '  This download has expired.\n' }, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: true } })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toBe('ToneDen said: This download has expired.');
	});

	it('asks for the account to be connected however ToneDen says it does not know the browser', async () => {
		// No gate lookup is stubbed, so the refusal's own reading is what the row shows.
		for (const refusal of [
			res({ message: 'Forbidden' }, { ok: false, status: 403 }),
			res(
				{ name: 'notAuthenticated', message: 'Please login to your account to continue.' },
				{ ok: false, status: 400 }
			)
		]) {
			net(routes({ '/api/v1/attachments/6201383/download': () => refusal }));
			expect(await message(toneden.unlock(POST, details()))).toMatch(/connect the account once/);
		}
	});

	it('reads a platform it has no name for with its hyphens as spaces', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ platform: 'tidal-save' }) }),
				'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: 42 })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(/wants tidal save\./);
	});

	it('names email like any other account when the gate wants more than an address', async () => {
		net(
			routes({
				'/api/v1/posts?': () =>
					res({ post: post({ platform: 'email', multi_action_enabled: true, platforms: ['email', 'spotify'] }) }),
				'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
				'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: 42 })
			})
		);
		expect(await message(toneden.unlock(POST, details()))).toMatch(/wants email and Spotify\./);
	});

	it('follows a redirect gate to a plain http address too', async () => {
		const url = 'http://www.dropbox.com/s/x/One.wav?dl=1';
		net(routes({ '/api/v1/posts?': () => res({ post: post({ download_source: 'redirect', direct_url: url }) }) }));
		expect(await toneden.unlock(POST, details())).toMatchObject({ kind: 'file', url });
	});

	it('falls through to the download when a redirect gate names something that is not a web address', async () => {
		net(
			routes({
				'/api/v1/posts?': () =>
					res({
						post: post({
							download_source: 'redirect',
							direct_url: 'www.dropbox.com/s/x/One.wav?dl=1&from=https://toneden.io'
						})
					})
			})
		);
		expect(await toneden.unlock(POST, details())).toMatchObject({ kind: 'file', url: FILE });
	});

	it('asks for the file of a gate that is not a redirect, whatever address the post carries', async () => {
		net(
			routes({
				'/api/v1/posts?': () => res({ post: post({ direct_url: 'https://www.beatport.com/release/never-felt/1' }) })
			})
		);
		expect(await toneden.unlock(POST, details())).toMatchObject({ kind: 'file', url: FILE });
	});

	it('takes a file name from the post, but never an address', async () => {
		const filename = async (display: string) => {
			net(routes({ '/api/v1/posts?': () => res({ post: post({ download_display_url: display }) }) }));
			const answer = await toneden.unlock(POST, details());

			return 'filename' in answer ? answer.filename : undefined;
		};

		expect(await filename('http://files.toneden.test/neverfelt.wav')).toBe('');
		expect(await filename('https://files.toneden.test/neverfelt.wav')).toBe('');
		// A name with an address inside it is still a name.
		expect(await filename('neverfelt (https://afterwes.test edit).wav')).toBe(
			'neverfelt (https://afterwes.test edit).wav'
		);
	});

	// Stryker switches a mutant on only once this file's imports have run, so
	// what the module builds as it loads (its schemas, serviceName, titleOf) is
	// checked here against a copy imported after that.
	describe('what the module builds as it loads', () => {
		async function fresh() {
			vi.resetModules();

			return (await import('../../src/tools/third-party-downloads/sites/toneden.ts')).toneden;
		}

		it('takes the file from the download answer and names it after the post', async () => {
			const site = await fresh();
			net(routes({ '/api/v1/posts?': () => res({ post: post({ author: undefined }) }) }));
			expect(await site.unlock(POST, details())).toEqual({
				kind: 'file',
				url: FILE,
				title: 'never felt',
				filename: 'neverfelt.wav'
			});
		});

		it('names the account a closed gate wants, for a browser ToneDen knows', async () => {
			const site = await fresh();
			net(
				routes({
					'/api/v1/posts?': () => res({ post: post({ platform: 'soundcloud' }) }),
					'/api/v1/attachments/6201383/download': () => res({}, { ok: false, status: 400 }),
					'/gateMetadata': () => res({ metadata: { is_gate_open: false }, viewer_id: 42 })
				})
			);
			expect(await message(site.unlock(POST, details()))).toMatch(
				/signed in to hasn’t been through this gate: it wants SoundCloud\./
			);
		});
	});
});

// --- FanLink -----------------------------------------------------------------

describe('FanLink', () => {
	const LINK = 'https://fanlink.tv/NPxBC-Nonstop';

	const page = (link: Json) => `<script>window.preloadLink = ${JSON.stringify(link)};</script>`;

	const link = (over = {}) => ({
		id: 3471055,
		title: 'NIK P & Black Carl! - Nonstop',
		author: 'NIK P & Black Carl!',
		target_url: 'https://open.spotify.com/track/3ogpvXigUn1DOPJQ03L6M2',
		rsvp_attachment: null,
		services: [
			{
				url: 'https://soundcloud.com/nikpmusic/nonstop',
				service_name: 'soundcloud'
			},
			{
				url: 'https://nikp.bandcamp.com/track/nonstop',
				service_name: 'bandcamp'
			}
		],
		...over
	});

	it('reads the smart link and offers the place a track is bought', async () => {
		net({ 'fanlink.tv/NPxBC-Nonstop': () => res(page(link())) });
		expect(await fanlink.unlock(LINK, details())).toEqual({
			kind: 'link',
			url: 'https://nikp.bandcamp.com/track/nonstop',
			title: 'NIK P & Black Carl! - Nonstop',
			why: 'a smart link, not a gate — it points at soundcloud, bandcamp.'
		});
	});

	it('saves a service that is a file outright', async () => {
		net({
			'fanlink.tv/': () =>
				res(
					page(
						link({
							services: [{ url: 'https://cdn.test/One.wav', service_name: 'custom' }]
						})
					)
				)
		});
		expect(await fanlink.unlock(LINK, details())).toMatchObject({ kind: 'file' });
	});

	it('falls back to where the link itself points, and then to the page', async () => {
		net({ 'fanlink.tv/': () => res(page(link({ services: undefined }))) });
		expect(await fanlink.unlock(LINK, details())).toMatchObject({
			kind: 'link',
			url: 'https://open.spotify.com/track/3ogpvXigUn1DOPJQ03L6M2',
			why: 'a smart link, not a gate: there’s no file behind it.'
		});

		net({
			'fanlink.tv/': () => res(page(link({ services: [], target_url: '' })))
		});
		expect((await fanlink.unlock(LINK, details())).url).toBe(LINK);
	});

	it('walks the ToneDen unlock a fanlink can carry', async () => {
		net({
			'fanlink.tv/': () =>
				res(
					page(
						link({
							rsvp_attachment: {
								id: 99,
								title: 'B-side',
								download_source: 'redirect',
								direct_url: 'https://cdn.test/Bside.mp3'
							}
						})
					)
				)
		});
		// The attachment names itself, and does it better than the link around it.
		expect(await fanlink.unlock(LINK, details())).toEqual({
			kind: 'file',
			url: 'https://cdn.test/Bside.mp3',
			title: 'B-side'
		});
	});

	it('names itself from whichever of title and author actually says something', () => {
		expect(fanLinkTitle({ id: 1, author: 'H V D E S', title: 'Bring Me To Life' })).toBe(
			'H V D E S — Bring Me To Life'
		);
		// The author is the page's description, with the title already inside it.
		expect(
			fanLinkTitle({
				id: 1,
				author: 'Listen and download Bring Me To Life by H V D E S',
				title: 'Bring Me To Life'
			})
		).toBe('Bring Me To Life');
		// And the other way round: a title that already names its artist.
		expect(fanLinkTitle({ id: 1, author: 'NIK P', title: 'NIK P - Nonstop' })).toBe('NIK P - Nonstop');
		expect(fanLinkTitle({ id: 1, author: 'H V D E S', title: '' })).toBe('H V D E S');
		expect(fanLinkTitle({ id: 1 })).toBe('');
	});

	it('rewrites the suffix and nothing else', () => {
		expect(fanLinkUrl('http://www.fanlink.to/a/b?c=d#e')).toBe('https://fanlink.tv/a/b?c=d#e');
		expect(fanLinkUrl('https://fanlink.tv/x')).toBe('https://fanlink.tv/x');
	});

	it('reads a dead fanlink.to link off fanlink.tv, where the paths came across', async () => {
		const calls = net({ 'fanlink.tv/NPxBC-Nonstop': () => res(page(link())) });
		expect(await fanlink.unlock('http://www.fanlink.to/NPxBC-Nonstop', details())).toMatchObject({
			url: 'https://nikp.bandcamp.com/track/nonstop'
		});
		expect(calls[0].url).toBe(LINK);
	});

	it('offers the rewritten address, not the dead one, when there is nowhere else', async () => {
		net({
			'fanlink.tv/': () => res(page(link({ services: [], target_url: '' })))
		});
		expect((await fanlink.unlock('https://fanlink.to/NPxBC-Nonstop', details())).url).toBe(LINK);
	});

	it('follows a link whose destination is a ToneDen gate rather than offering it', async () => {
		const POST = 'https://www.toneden.io/hvdesmusic/post/bring-me-to-life';
		net({
			'fanlink.tv/': () =>
				res(
					page(
						link({
							title: 'Evanescence - Bring Me To Life (HVDES Remix)',
							// What a FanLink puts in `author` is not always an artist.
							author: 'Listen and download Evanescence - Bring Me To Life (HVDES Remix) by H V D E S',
							services: [],
							target_url: POST
						})
					)
				),
			'/api/v1/posts?': () =>
				res({
					post: {
						id: 7,
						title: 'Bring Me To Life (HVDES Remix)',
						author: { display_name: 'H V D E S' },
						download_source: 'redirect',
						direct_url: 'https://cdn.test/hvdes.wav'
					}
				})
		});
		expect(await fanlink.unlock(LINK, details())).toEqual({
			kind: 'file',
			url: 'https://cdn.test/hvdes.wav',
			// ToneDen's own naming wins over the FanLink's description-shaped author.
			title: 'H V D E S — Bring Me To Life (HVDES Remix)'
		});
	});

	it('ignores a destination that is an app URI rather than a web address', async () => {
		net({
			'fanlink.tv/': () =>
				res(
					page(
						link({
							services: [],
							target_url: 'spotify:album:4yPvemc8nP0odWoFjFoPbV'
						})
					)
				)
		});
		expect((await fanlink.unlock(LINK, details())).url).toBe(LINK);
	});

	it('reports a page it cannot read', async () => {
		net({ 'fanlink.tv/': () => res('<html></html>') });
		expect(await message(fanlink.unlock(LINK, details()))).toMatch(/not a ToneDen FanLink/);

		net({ 'fanlink.tv/': () => res('', { ok: false, status: 404 }) });
		expect(await message(fanlink.unlock(LINK, details()))).toMatch(/HTTP 404/);
	});

	it('takes the global however the page spaces it', async () => {
		net({
			'fanlink.tv/': () => res(`<script>window.preloadLink={"id":1,"services":[]}</script>`)
		});
		expect((await fanlink.unlock(LINK, details())).kind).toBe('link');
	});

	it('does not mistake an unnamed service for a shop', async () => {
		net({
			'fanlink.tv/': () =>
				res(
					page(
						link({
							services: [{ url: 'https://mystery.test/1' }],
							target_url: ''
						})
					)
				)
		});
		expect(await fanlink.unlock(LINK, details())).toMatchObject({
			url: LINK,
			why: 'a smart link, not a gate: there’s no file behind it.'
		});
	});

	it('passes over a service with no address, rather than naming it', async () => {
		net({
			'fanlink.tv/': () => res(page(link({ services: [{ service_name: 'bandcamp' }], target_url: '' })))
		});
		expect(await fanlink.unlock(LINK, details())).toMatchObject({
			url: LINK,
			why: 'a smart link, not a gate: there’s no file behind it.'
		});
	});

	it('asks for the page as HTML, with the browser’s session', async () => {
		const calls = net({ 'fanlink.tv/': () => res(page(link())) });
		await fanlink.unlock(LINK, details());
		expect(calls[0].options).toEqual({ credentials: 'include', headers: { Accept: 'text/html' } });
	});

	it('finds the global past other JSON on the page, however it is spaced', async () => {
		// Structured data in the head comes before the app's own state.
		const head = '<script type="application/ld+json">{"@type":"MusicRecording","name":"Nonstop"}</script>';

		for (const global of ['window.preloadLink = ', 'window.preloadLink=']) {
			net({ 'fanlink.tv/': () => res(`${head}<script>${global}${JSON.stringify(link())};</script>`) });
			expect((await fanlink.unlock(LINK, details())).url).toBe('https://nikp.bandcamp.com/track/nonstop');
		}
	});

	it('reads a link without an id as no link at all', async () => {
		net({ 'fanlink.tv/': () => res(page(link({ id: 0 }))) });
		expect(await message(fanlink.unlock(LINK, details()))).toMatch(/not a ToneDen FanLink/);
	});

	it('trims the title and author before putting them together', () => {
		expect(fanLinkTitle({ id: 1, author: ' H V D E S\n', title: '  Bring Me To Life ' })).toBe(
			fanLinkTitle({ id: 1, author: 'H V D E S', title: 'Bring Me To Life' })
		);
		expect(fanLinkTitle({ id: 1, author: 'H V D E S', title: '   ' })).toBe('H V D E S');
	});

	it('offers a destination that only looks like a ToneDen gate, rather than looking it up', async () => {
		// Nothing on toneden.io is stubbed, so a lookup would fail the unlock.
		for (const target of [
			'https://www.toneden.io/hvdesmusic',
			'https://hvdes.test/blog/post/bring-me-to-life',
			'https://toneden.www.io/hvdesmusic/post/bring-me-to-life'
		]) {
			net({ 'fanlink.tv/': () => res(page(link({ services: [], target_url: target }))) });
			expect((await fanlink.unlock(LINK, details())).url).toBe(target);
		}
	});

	it('takes a destination over plain http, but not an app URI with an address inside it', async () => {
		net({ 'fanlink.tv/': () => res(page(link({ services: [], target_url: 'http://open.spotify.com/track/1' }))) });
		expect((await fanlink.unlock(LINK, details())).url).toBe('http://open.spotify.com/track/1');

		net({
			'fanlink.tv/': () =>
				res(page(link({ services: [], target_url: 'spotify:track:1?referrer=https://fanlink.tv/NPxBC-Nonstop' })))
		});
		expect((await fanlink.unlock(LINK, details())).url).toBe(LINK);
	});
});

// --- Pump Your Sound ---------------------------------------------------------

describe('Pump Your Sound', () => {
	const GATE = 'https://pumpyoursound.com/f/pys/love-me-now/197974';
	const FILE = 'https://s3.test/pumpyoursound/tracks/love-me-now.mp3';
	// The form's own address, which is not the page's: the signal is in `do`.
	const ACTION = '/f/pys/love-me-now/197974?do=freeDownloadForm-submit';

	const page = (over = '') => `
    <h1 class="fangateTitle">LOVE ME NOW</h1>
    <form action="${ACTION}" method="post" id="frm-freeDownloadForm">
      <input name="fangate_comment" value="Really amazing track!">
      <input type="hidden" name="_do" value="freeDownloadForm-submit">
      <input type="submit" name="send" value="Download">
      ${over}
    </form>`;

	it('submits the form the Download button submits, and takes the file it lands on', async () => {
		const calls = net({
			'pumpyoursound.com': (options) => (options.method === 'POST' ? res('', { url: FILE }) : res(page()))
		});

		expect(await pumpyoursound.unlock(GATE, details())).toEqual({
			kind: 'file',
			url: FILE,
			title: 'LOVE ME NOW'
		});
		// Where the form says, resolved against the page, not the page itself.
		expect(calls[1].url).toBe(new URL(ACTION, GATE).href);
		expect(form(calls, 'pumpyoursound.com')).toEqual({
			fangate_comment: 'Really amazing track!',
			_do: 'freeDownloadForm-submit',
			send: 'Download'
		});
	});

	it('finds the link in the page it comes back to instead', async () => {
		net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST' ? res(`<a href="${FILE}">Your download</a>`, { url: GATE }) : res(page())
		});
		expect(await pumpyoursound.unlock(GATE, details())).toMatchObject({
			kind: 'file',
			url: FILE
		});
	});

	it('says what to do when the form comes back with nothing on it', async () => {
		net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST' ? res('<p>follow first</p>', { url: GATE }) : res(page())
		});
		const said = await message(pumpyoursound.unlock(GATE, details()));
		expect(said).toMatch(/handed back nothing/);
		// And, to end on, where to go instead.
		expect(said).toMatch(/open the gate and try there\.$/);
	});

	it('presses a Download button the page did not name', async () => {
		const calls = net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST' ? res('', { url: FILE }) : res(page().replace('name="send" ', ''))
		});

		await pumpyoursound.unlock(GATE, details());
		expect(form(calls, 'pumpyoursound.com')).toEqual({
			fangate_comment: 'Really amazing track!',
			_do: 'freeDownloadForm-submit'
		});
	});

	it('presses a Download button written as a <button>, by the name the page gave it', async () => {
		const calls = net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST'
					? res('', { url: FILE })
					: res(
							page().replace(
								'<input type="submit" name="send" value="Download">',
								'<button name="send" value="Download">Download</button>'
							)
						)
		});

		await pumpyoursound.unlock(GATE, details());
		expect(form(calls, 'pumpyoursound.com')).toEqual({
			fangate_comment: 'Really amazing track!',
			_do: 'freeDownloadForm-submit',
			send: 'Download'
		});
	});

	it('posts back to the page itself when the form names nowhere', async () => {
		const calls = net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST' ? res('', { url: FILE }) : res(page().replace(`action="${ACTION}" `, ''))
		});

		await pumpyoursound.unlock(GATE, details());
		expect(calls[1].url).toBe(GATE);
	});

	it('reports a page with no fangate on it', async () => {
		net({ 'pumpyoursound.com': () => res('<html></html>') });
		expect(await message(pumpyoursound.unlock(GATE, details()))).toMatch(/not a Pump Your Sound fangate/);

		net({ 'pumpyoursound.com': () => res('', { ok: false, status: 404 }) });
		expect(await message(pumpyoursound.unlock(GATE, details()))).toMatch(/HTTP 404/);

		net({
			'pumpyoursound.com': (options) => (options.method === 'POST' ? res('', { ok: false, status: 500 }) : res(page()))
		});
		expect(await message(pumpyoursound.unlock(GATE, details()))).toMatch(/answered 500/);
	});

	it('walks a fangate with no title on it', async () => {
		net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST' ? res('', { url: FILE }) : res(page().replace(/<h1[^>]*>.*<\/h1>/, ''))
		});
		expect(await pumpyoursound.unlock(GATE, details())).toMatchObject({ title: '' });
	});

	it('trims the title the page sets out on lines of its own', async () => {
		net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST'
					? res('', { url: FILE })
					: res(page().replace('>LOVE ME NOW<', '>\n      LOVE ME NOW\n    <'))
		});
		expect(await pumpyoursound.unlock(GATE, details())).toMatchObject({ title: 'LOVE ME NOW' });
	});

	it('asks for the page and posts the form with the visitor’s cookies, as the browser would', async () => {
		// The steps beside the button are walked in a tab, and their cookies are
		// what the server checks the form against.
		const calls = net({
			'pumpyoursound.com': (options) => (options.method === 'POST' ? res('', { url: FILE }) : res(page()))
		});

		await pumpyoursound.unlock(GATE, details());
		expect(calls[0].options).toEqual({ credentials: 'include', headers: { Accept: 'text/html' } });
		expect(calls[1].options).toEqual({
			method: 'POST',
			credentials: 'include',
			headers: { 'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8' },
			body: expect.any(String)
		});
	});

	it('presses only a button, not another control whose type reads submit', async () => {
		const calls = net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST'
					? res('', { url: FILE })
					: res(
							page().replace(
								'<input type="submit"',
								'<object name="player" type="submit"></object><input type="submit"'
							)
						)
		});

		await pumpyoursound.unlock(GATE, details());
		expect(form(calls, 'pumpyoursound.com')).toEqual({
			fangate_comment: 'Really amazing track!',
			_do: 'freeDownloadForm-submit',
			send: 'Download'
		});
	});

	it('reads the answer when the form posts to an address that looks like a file and stays there', async () => {
		// Nothing redirected, so what came back is the page again, whatever its address says.
		const action = 'https://pumpyoursound.com/downloads/love-me-now.zip';
		net({
			'pumpyoursound.com': (options) =>
				options.method === 'POST'
					? res(`<a href="${FILE}">Your download</a>`, { url: action })
					: res(page().replace(`action="${ACTION}"`, `action="${action}"`))
		});
		expect(await pumpyoursound.unlock(GATE, details())).toMatchObject({ kind: 'file', url: FILE });
	});

	it('lets go of the file it lands on rather than reading it', async () => {
		// The download fetches the file afresh, so there is no reason to pull it down twice.
		let cancelled = false;

		const bytes = new ReadableStream({
			cancel: () => {
				cancelled = true;
			}
		});

		// Real responses, so the body is a real stream. The form lands on the file.
		const serve = (body: ReadableStream | null) => {
			const landed = Object.defineProperty(new Response(body), 'url', { value: FILE });

			vi.stubGlobal(
				'fetch',
				vi.fn<typeof fetch>(async (_url, init) => (init?.method === 'POST' ? landed : new Response(page())))
			);
		};

		serve(bytes);
		expect(await pumpyoursound.unlock(GATE, details())).toMatchObject({ kind: 'file', url: FILE });
		expect(cancelled).toBe(true);

		// An answer with no body at all has nothing to let go of.
		serve(null);
		expect(await pumpyoursound.unlock(GATE, details())).toMatchObject({ kind: 'file', url: FILE });
	});
});

// --- Laylo -------------------------------------------------------------------

describe('Laylo', () => {
	const DROP = 'https://laylo.com/hairitage/9PWcf';
	const JSON_URL = 'https://d21i0hc4hl3bvt.cloudfront.net/hairitage/9PWcf.json';
	const USER_URL = 'https://d3oyaxbt9vo0fg.cloudfront.net/users/abc123.json';
	const SIGNUP = 'https://events.laylo.com/actions/rsvp/rsvp';
	const ME = { email: 'me@example.com', name: 'Sam' };

	// The drop page is a shell that fetches the drop, then the artist behind it.
	const page = (url = JSON_URL, users = 'https://d3oyaxbt9vo0fg.cloudfront.net') =>
		`<script>fetch('${url}', { method: "GET" });` +
		`fetch('${users}' + '/users/' + userId + '.json', { method: "GET" })</script>`;

	const drop = (over = {}) => ({
		title: 'HAIRITAGE REMIXES (FREE DL)',
		link: null,
		productId: 'aeb82e40-6224-4b3f-81b2-85b92779a219',
		endDate: null,
		doubleOptIn: false,
		additionalRSVPInputs: [],
		rsvpOptions: { sms: 0, instagram: -1, email: 1 },
		user: { id: 'abc123', username: 'hairitage' },
		...over
	});

	const artist = (over = {}) => ({
		id: 'abc123',
		displayName: 'Hairitage',
		...over
	});

	const taken = () => res({ success: true, userId: 'u1', postRSVPUpsellConfigs: [] });

	/** A drop that is collecting an address, and an artist to collect it for. */
	const offering = (dropOver = {}, artistOver = {}) => ({
		'//laylo.com/': () => res(page()),
		d21i0hc4hl3bvt: () => res(drop(dropOver)),
		d3oyaxbt9vo0fg: () => res(artist(artistOver))
	});

	it('signs the address from settings up, the way the page does', async () => {
		const calls = net({ ...offering(), 'events.laylo.com': () => taken() });
		expect(await laylo.unlock(DROP, ME)).toEqual({
			kind: 'signup',
			url: DROP,
			title: 'Hairitage — HAIRITAGE REMIXES (FREE DL)',
			why: 'signed me@example.com up — Hairitage sends the link out by mail.'
		});

		const { type, payload } = sent(calls, SIGNUP);
		expect(type).toBe('rsvp');
		expect(payload).toMatchObject({
			productId: 'aeb82e40-6224-4b3f-81b2-85b92779a219',
			// The one field the endpoint refuses outright without.
			creatorId: 'abc123',
			email: 'me@example.com',
			optIn: true,
			name: 'Sam',
			firstName: 'Sam'
		});
		// A fresh one each time, and never empty: the endpoint wants something.
		expect(payload.fingerprintId).toMatch(/^[0-9a-f]{32}$/);
		// No captcha token is forged. The field is optional; a fake one is refused.
		expect(payload).not.toHaveProperty('captcha');
	});

	it('says to confirm it when the artist asks Laylo to check the address', async () => {
		const calls = net({
			...offering({}, { requireEmailVerify: true }),
			'events.laylo.com': () => taken()
		});

		expect(why(await laylo.unlock(DROP, ME))).toMatch(/confirm it in the mail Laylo sends/);
		expect(sent(calls, SIGNUP).payload.optIn).toBe(false);
	});

	it('asks for the address rather than walking off to the drop', async () => {
		net(offering());
		const error = await message(laylo.unlock(DROP, details()));
		expect(error).toMatch(/add an address in settings/);

		net(offering());

		await expect(laylo.unlock(DROP, details())).rejects.toMatchObject({ next: 'email' });
	});

	it('asks for a name too, when the drop is one that wants one', async () => {
		const asking = {
			additionalRSVPInputs: [{ fieldName: 'name', required: true }]
		};

		net(offering(asking));

		await expect(laylo.unlock(DROP, details({ email: 'me@example.com' }))).rejects.toMatchObject({
			next: 'name',
			message: expect.stringMatching(/asks for a name/)
		});

		// With one to hand it is just another field on the signup.
		const calls = net({
			...offering(asking),
			'events.laylo.com': () => taken()
		});

		expect((await laylo.unlock(DROP, ME)).kind).toBe('signup');
		expect(sent(calls, SIGNUP).payload.firstName).toBe('Sam');
	});

	it('says what happened when Laylo turns the signup down', async () => {
		net({ ...offering(), 'events.laylo.com': () => res({ success: false }) });
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/turned the signup down/);

		net({
			...offering(),
			'events.laylo.com': () => res({}, { ok: false, status: 502 })
		});
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/answered 502 to the signup/);
	});

	it('leaves alone the drops it has nothing to offer', async () => {
		// Only a phone number: this has none to give.
		net(offering({ rsvpOptions: { sms: 0, email: -1 } }));
		expect(await laylo.unlock(DROP, ME)).toMatchObject({
			kind: 'link',
			url: DROP,
			why: expect.stringMatching(/takes a phone number and the artist sends the link out/)
		});

		// Confirmed by text, which Laylo only does over SMS.
		net(offering({ doubleOptIn: true }));
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/confirms signups by text/);

		// Questions with no answer in settings.
		net(
			offering({
				additionalRSVPInputs: [{ fieldName: 'birthday', required: true }]
			})
		);
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/asks for birthday as well/);

		// Required the way the page reads it, by any truthy value, past an input it cannot read.
		net(
			offering({
				additionalRSVPInputs: [null, { fieldName: 'birthday', required: 'true' }]
			})
		);
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/asks for birthday as well/);

		// Nothing at all on offer.
		net(offering({ rsvpOptions: undefined }));
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/takes a signup/);
	});

	it('names every channel a drop is collecting, in the order it offers them', async () => {
		// The numbers are that order, not a count, and -1 is not offered at all.
		net(offering({ rsvpOptions: { email: -1, sms: 1, instagram: 0 } }));
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/takes an Instagram follow or a phone number/);

		// An artist's own options stand in for the drop's, the way the page reads
		// them: here, taking email off a drop that offered it.
		net(offering({ rsvpOptions: { email: 0 } }, { rsvpOptions: { sms: 0, email: -1 } }));
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/takes a phone number/);
	});

	it('follows a drop that does point somewhere, and signs nothing up', async () => {
		const calls = net({
			'//laylo.com/': () => res(page()),
			d21i0hc4hl3bvt: () => res(drop({ link: 'https://cdn.test/pack.zip' })),
			d3oyaxbt9vo0fg: () => res(artist())
		});

		expect(await laylo.unlock(DROP, ME)).toMatchObject({ kind: 'file' });
		expect(calls.some((call) => new URL(call.url).hostname === 'events.laylo.com')).toBe(false);

		net({
			'//laylo.com/': () => res(page()),
			d21i0hc4hl3bvt: () => res(drop({ link: 'https://shop.test/buy' })),
			d3oyaxbt9vo0fg: () => res(artist())
		});
		expect(await laylo.unlock(DROP, ME)).toMatchObject({
			kind: 'link',
			url: 'https://shop.test/buy',
			why: 'the drop points here.'
		});
	});

	it('works both URLs out itself when the page does not name them', async () => {
		const calls = net({
			'//laylo.com/': () => res('<html></html>'),
			d21i0hc4hl3bvt: () => res(drop()),
			d3oyaxbt9vo0fg: () => res(artist()),
			'events.laylo.com': () => taken()
		});

		await laylo.unlock(DROP, ME);
		expect(calls[1].url).toBe(JSON_URL);
		expect(calls[2].url).toBe(USER_URL);
	});

	it('reports a page, a drop and an artist that will not load', async () => {
		net({ '//laylo.com/': () => res('', { ok: false, status: 404 }) });
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/HTTP 404/);

		net({ '//laylo.com/': () => res('<html></html>') });
		expect(await message(laylo.unlock('https://laylo.com/', ME))).toMatch(/not a Laylo drop/);

		net({
			'//laylo.com/': () => res(page()),
			d21i0hc4hl3bvt: () => res({}, { ok: false, status: 403 })
		});
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/answered 403 for that drop/);

		net({
			'//laylo.com/': () => res(page()),
			d21i0hc4hl3bvt: () => res(drop()),
			d3oyaxbt9vo0fg: () => res({}, { ok: false, status: 500 })
		});
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/answered 500 for that artist/);
	});

	it('says so when a drop has moved somewhere the extension cannot reach', async () => {
		net({
			'//laylo.com/': () => res(page('https://elsewhere.test/a.json')),
			'elsewhere.test': () => {
				throw new TypeError('Failed to fetch');
			}
		});
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/elsewhere\.test.*can’t reach/s);

		net({
			...offering(),
			'events.laylo.com': () => {
				throw new TypeError('Failed to fetch');
			}
		});
		expect(await message(laylo.unlock(DROP, ME))).toMatch(/signup wouldn’t answer/);
	});

	it('drops a title for a drop that names nobody', async () => {
		net({
			'//laylo.com/': () => res(page()),
			d21i0hc4hl3bvt: () => res(drop({ user: null, title: '' })),
			'events.laylo.com': () => taken()
		});
		const walked = await laylo.unlock(DROP, ME);
		expect(walked.title).toBe('');
		// With no artist on it there is nobody to sign up with, either.
		expect(walked.kind).toBe('link');
	});

	it('reads the artist CDN off the page rather than assuming Laylo’s own', async () => {
		const calls = net({
			'//laylo.com/': () => res(page(JSON_URL, 'https://users.elsewhere.test')),
			d21i0hc4hl3bvt: () => res(drop()),
			'users.elsewhere.test': () => res(artist()),
			'events.laylo.com': () => taken()
		});

		expect((await laylo.unlock(DROP, ME)).kind).toBe('signup');
		expect(calls[2].url).toBe('https://users.elsewhere.test/users/abc123.json');

		// Pages have also written the path onto the base rather than adding it.
		const whole = net({
			'//laylo.com/': () =>
				res(`<script>fetch('${JSON_URL}');fetch('https://users.elsewhere.test/users/' + userId + '.json')</script>`),
			d21i0hc4hl3bvt: () => res(drop()),
			'users.elsewhere.test': () => res(artist()),
			'events.laylo.com': () => taken()
		});

		expect((await laylo.unlock(DROP, ME)).kind).toBe('signup');
		expect(whole[2].url).toBe('https://users.elsewhere.test/users/abc123.json');
	});

	it('names a channel Laylo has invented since in the drop’s own word for it', async () => {
		net(offering({ rsvpOptions: { whatsapp: 0, email: -1 } }));
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/takes whatsapp and the artist/);
	});

	it('says “the artist” when the drop’s artist has no name to put on it', async () => {
		net({
			...offering({ user: { id: 'abc123' } }, { displayName: '' }),
			'events.laylo.com': () => taken()
		});
		expect(why(await laylo.unlock(DROP, ME))).toBe('signed me@example.com up — the artist sends the link out by mail.');

		net({
			...offering({ user: { id: 'abc123' } }, { displayName: '', requireEmailVerify: true }),
			'events.laylo.com': () => taken()
		});
		expect(why(await laylo.unlock(DROP, ME))).toBe(
			'signed me@example.com up — confirm it in the mail Laylo sends, and the artist sends the link out.'
		);
	});

	it('reads both URLs off a page however its fetches are spaced', async () => {
		// Line breaks inside the calls, and no spaces at all around the `+`, as a minifier leaves it.
		const calls = net({
			'//laylo.com/': () =>
				res(
					'<script>fetch(\n  "https://drops.elsewhere.test/hairitage/9PWcf.json");' +
						'fetch(\n  "https://users.elsewhere.test"+"/users/"+e+".json")</script>'
				),
			'drops.elsewhere.test': () => res(drop()),
			'users.elsewhere.test': () => res(artist()),
			'events.laylo.com': () => taken()
		});

		expect((await laylo.unlock(DROP, ME)).kind).toBe('signup');
		expect(calls[1].url).toBe('https://drops.elsewhere.test/hairitage/9PWcf.json');
		expect(calls[2].url).toBe('https://users.elsewhere.test/users/abc123.json');
	});

	it('works the drop out from a link with slashes left on the end', async () => {
		const calls = net({
			'//laylo.com/': () => res('<html></html>'),
			d21i0hc4hl3bvt: () => res(drop()),
			d3oyaxbt9vo0fg: () => res(artist()),
			'events.laylo.com': () => taken()
		});

		await laylo.unlock(`${DROP}//`, ME);
		expect(calls[1].url).toBe(JSON_URL);
	});

	it('asks for the page, the drop, the artist and the signup the way the page itself does', async () => {
		const calls = net({ ...offering(), 'events.laylo.com': () => taken() });

		await laylo.unlock(DROP, ME);
		expect(calls[0].options).toEqual({ headers: { Accept: 'text/html' } });
		expect(calls[1].options).toEqual({ headers: { Accept: 'application/json' } });
		expect(calls[2].options).toEqual({ headers: { Accept: 'application/json' } });
		expect(calls[3].options).toMatchObject({ method: 'POST', headers: { 'Content-Type': 'application/json' } });
	});

	it('makes up a fresh fingerprint for every signup', async () => {
		// Loaded afresh, so the maker is built inside the test, where a mutation
		// run can reach it.
		vi.resetModules();
		const { default: fresh } = await import('../../src/tools/third-party-downloads/sites/laylo.ts');
		const calls = net({ ...offering(), 'events.laylo.com': () => taken() });

		await fresh.unlock(DROP, ME);
		await fresh.unlock(DROP, ME);
		const [one, two] = calls.filter((call) => call.url === SIGNUP).map((call) => JSON.parse(call.body).payload);
		expect(one.fingerprintId).toMatch(/^[0-9a-f]{32}$/);
		expect(two.fingerprintId).toMatch(/^[0-9a-f]{32}$/);
		expect(one.fingerprintId).not.toBe(two.fingerprintId);
	});

	it('passes the drop’s end date on with the signup, or null for a drop with none', async () => {
		const calls = net({ ...offering({ endDate: '2026-12-01T00:00:00.000Z' }), 'events.laylo.com': () => taken() });

		await laylo.unlock(DROP, ME);
		expect(sent(calls, SIGNUP).payload.dropDate).toBe('2026-12-01T00:00:00.000Z');

		const none = net({ ...offering({ endDate: undefined }), 'events.laylo.com': () => taken() });

		await laylo.unlock(DROP, ME);
		expect(sent(none, SIGNUP).payload.dropDate).toBe(null);
	});

	it('signs up with the address alone when settings holds no name', async () => {
		const calls = net({ ...offering(), 'events.laylo.com': () => taken() });

		await laylo.unlock(DROP, details({ email: 'me@example.com' }));
		expect(sent(calls, SIGNUP).payload).not.toHaveProperty('name');
		expect(sent(calls, SIGNUP).payload).not.toHaveProperty('firstName');
	});

	it('names every question a drop asks that settings cannot answer', async () => {
		net(
			offering({
				additionalRSVPInputs: [
					{ fieldName: 'birthday', required: true },
					{ fieldName: 'city', required: true }
				]
			})
		);
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/asks for birthday and city as well/);
	});

	it('ends a notify-me row on what to do about it', async () => {
		net(offering({ rsvpOptions: { sms: 0, email: -1 } }));
		expect(why(await laylo.unlock(DROP, ME))).toMatch(/open it and sign up if you want it\.$/);
	});

	it('keeps the network error underneath a failure to reach Laylo', async () => {
		const refused = new TypeError('Failed to fetch');

		net({
			'//laylo.com/': () => res(page('https://elsewhere.test/a.json')),
			'elsewhere.test': () => {
				throw refused;
			}
		});
		await expect(laylo.unlock(DROP, ME)).rejects.toHaveProperty('cause', refused);

		net({
			...offering(),
			'events.laylo.com': () => {
				throw refused;
			}
		});
		await expect(laylo.unlock(DROP, ME)).rejects.toHaveProperty('cause', refused);
	});
});

// --- Feature.fm --------------------------------------------------------------

describe('Feature.fm', () => {
	const LINK = 'https://ffm.to/1111-sos';

	const button = (srvc: string | undefined, destUrl: string) =>
		`<a href="https://api.ffm.to/sl/e/c/1111-sos?cd=${btoa(JSON.stringify({ srvc, destUrl }))
			.replace(/\+/g, '-')
			.replace(/\//g, '_')
			.replace(/=+$/, '')}">${srvc}</a>`;

	const page = (...buttons: string[]) => `<meta property="og:title" content="1111 - SOS">${buttons.join('')}`;

	it('reads each destination out of the tracker without clicking it', async () => {
		const calls = net({
			'ffm.to': () =>
				res(
					page(
						button('spotify', 'https://open.spotify.com/track/1'),
						button('beatport', 'https://www.beatport.com/release/sos/1')
					)
				)
		});

		expect(await featurefm.unlock(LINK, details())).toEqual({
			kind: 'link',
			url: 'https://www.beatport.com/release/sos/1',
			title: '1111 - SOS',
			why: 'a smart link, not a gate — it points at spotify, beatport.'
		});
		// The page, and nothing else: api.ffm.to counts what it is asked for.
		expect(calls).toHaveLength(1);
	});

	it('prefers a file, then Bandcamp, over anything else', async () => {
		net({
			'ffm.to': () => res(page(button('itunes', 'https://itunes.test/1'), button('custom', 'https://c.test/a.mp3')))
		});
		expect(await featurefm.unlock(LINK, details())).toMatchObject({
			kind: 'file',
			url: 'https://c.test/a.mp3'
		});

		net({
			'ffm.to': () => res(page(button('beatport', 'https://bp.test/1'), button('bandcamp', 'https://bc.test/1')))
		});
		expect((await featurefm.unlock(LINK, details())).url).toBe('https://bc.test/1');
	});

	it('offers the page itself when nothing on it can be bought', async () => {
		net({ 'ffm.to': () => res(page(button('youtube', 'https://youtu.be/1'))) });
		expect((await featurefm.unlock(LINK, details())).url).toBe(LINK);
	});

	it('asks over https even for a link written down as http', async () => {
		const calls = net({
			'ffm.to': () => res(page(button('spotify', 'https://s.test/1')))
		});

		await featurefm.unlock('http://ffm.to/1111-sos', details());
		expect(calls[0].url).toBe(LINK);
	});

	it('skips a payload it cannot read, and repeats of one it can', async () => {
		net({
			'ffm.to': () =>
				res(
					`<a href="https://api.ffm.to/sl/e/c/x?cd=!!!!">bad</a>` +
						`<a href="https://api.ffm.to/sl/e/c/x?cd=${btoa('{}')}">empty</a>` +
						button(undefined, 'https://unnamed.test/1') +
						button('spotify', 'https://s.test/1') +
						button('spotify', 'https://s.test/1')
				)
		});
		expect(why(await featurefm.unlock(LINK, details()))).toBe('a smart link, not a gate — it points at spotify.');
	});

	it('reports a page with nothing on it, and one that will not load', async () => {
		net({ 'ffm.to': () => res('<html></html>') });
		expect(await message(featurefm.unlock(LINK, details()))).toMatch(/nothing on that feature.fm page/);

		net({ 'ffm.to': () => res('', { ok: false, status: 404 }) });
		expect(await message(featurefm.unlock(LINK, details()))).toMatch(/HTTP 404/);
	});

	it('carries on without a title', async () => {
		net({ 'ffm.to': () => res(button('spotify', 'https://s.test/1')) });
		expect((await featurefm.unlock(LINK, details())).title).toBe('');
	});

	it('reads the title from a meta tag with other attributes around it', async () => {
		net({
			'ffm.to': () =>
				res(
					`<meta data-rh="true" property="og:title" itemprop="name" content="1111 - SOS">` +
						button('spotify', 'https://s.test/1')
				)
		});
		expect((await featurefm.unlock(LINK, details())).title).toBe('1111 - SOS');
	});

	it('keeps a destination over plain http, and skips one that is not a web address', async () => {
		net({
			'ffm.to': () =>
				res(
					page(
						// A share button: an address inside it, but not one to open.
						button('email', 'mailto:?body=https://ffm.to/1111-sos'),
						button('beatport', 'http://www.beatport.com/release/sos/1')
					)
				)
		});
		const answer = await featurefm.unlock(LINK, details());
		expect(answer.url).toBe('http://www.beatport.com/release/sos/1');
		expect(why(answer)).toMatch(/it points at beatport\.$/);
	});

	it('asks for the page as HTML', async () => {
		const calls = net({ 'ffm.to': () => res(page(button('spotify', 'https://s.test/1'))) });
		await featurefm.unlock(LINK, details());
		expect(calls[0].options).toEqual({ headers: { Accept: 'text/html' } });
	});
});

describe('every adapter', () => {
	it('throws rather than answering when the site cannot be reached at all', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn<typeof fetch>(async () => {
				throw new TypeError('Failed to fetch');
			})
		);

		for (const site of [hypeddit, droploud, toneden, pumpyoursound, featurefm, fanlink]) {
			await expect(site.unlock(`https://${site.hosts[0]}/a/post/b`, details())).rejects.toThrow(Error);
		}
	});
});
