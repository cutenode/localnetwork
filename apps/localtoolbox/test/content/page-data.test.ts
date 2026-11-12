// page-data.ts: what the page's own requests to SoundCloud's API hand to the
// content script. It runs in the page's world and only listens: these tests
// make the page's requests themselves and play SoundCloud's answer back on the
// request (a `load` with a status and a body), since jsdom has no network to
// send them over and the script never reads anything else.
import { describe, it, expect, afterEach, beforeEach } from 'vitest';
import { loadPageData, type ContentScript } from '../helpers/load.ts';

const API = 'https://api-v2.soundcloud.com';

const EVENT = 'tbx:page-data';

const HELLO = 'tbx:page-data-hello';

let script: ContentScript | undefined;

let heard: string[] = [];

const hear = (event: Event) => heard.push(event instanceof CustomEvent ? event.detail : '');

// What reached the original `open` (the one the page would have called), as
// the page called it: page-data.ts must pass every argument on as it came.
let opened: unknown[][] = [];

const realOpen = Object.getOwnPropertyDescriptor(XMLHttpRequest.prototype, 'open')!;

beforeEach(() => {
	heard = [];
	opened = [];
	document.addEventListener(EVENT, hear);
	XMLHttpRequest.prototype.open = function (this: XMLHttpRequest, ...args: unknown[]) {
		opened.push(args);
	};
});

afterEach(() => {
	script?.dispose();
	script = undefined;
	document.removeEventListener(EVENT, hear);
	Object.defineProperty(XMLHttpRequest.prototype, 'open', realOpen);
});

const load = async () => {
	script = await loadPageData();
};

const hello = () => document.dispatchEvent(new CustomEvent(HELLO));

interface Answer {
	method?: string;
	status?: number;
	body?: string;
	responseType?: XMLHttpRequestResponseType;
}

/** The page asks for `url`, and SoundCloud answers it. */
const request = (url: string | URL, { method = 'GET', status = 200, body = '[]', responseType = '' }: Answer = {}) => {
	const xhr = new XMLHttpRequest();
	xhr.open(method, url);
	// The fields a finished request reports, as its own properties: the
	// prototype's getters would read jsdom's state, and nothing was sent.
	Object.defineProperties(xhr, {
		status: { value: status },
		responseText: { value: body },
		responseType: { value: responseType }
	});
	xhr.dispatchEvent(new Event('load'));
};

describe('what is passed on', () => {
	it.each([
		['/tracks?ids=1,2,3&client_id=x', '/tracks'],
		['/resolve?url=https%3A%2F%2Fsoundcloud.com%2Fa%2Fb', '/resolve'],
		['/playlists/123?representation=full', '/playlists/123'],
		['/users/42', '/users/42'],
		['/users/soundcloud:users:42/likes', '/users/soundcloud:users:42/likes'],
		['/users/42/track_likes?limit=24', '/users/42/track_likes'],
		['/users/soundcloud:users:42/web-profiles', '/users/soundcloud:users:42/web-profiles']
	])('hands over %s by its path, with the body as SoundCloud sent it', async (url, path) => {
		await load();
		hello();
		request(`${API}${url}`, { body: '{"kind":"track"}' });
		expect(heard).toEqual([`${path}\n{"kind":"track"}`]);
	});

	it('takes a URL object, a lower-case method, and a text response, as the page may send them', async () => {
		await load();
		hello();
		request(new URL(`${API}/tracks`), { method: 'get', responseType: 'text', body: '[1]' });
		expect(heard).toEqual(['/tracks\n[1]']);
	});
});

describe('what is let by', () => {
	it.each([
		['a POST', `${API}/tracks`, { method: 'POST' }],
		['another host', 'https://api.soundcloud.com/tracks', {}],
		['a relative URL, which is the page’s own host', '/tracks', {}],
		['an answer it does not read', `${API}/tracks/1/comments`, {}],
		['a playlist named by something other than its id', `${API}/playlists/soundcloud:system-playlists:x`, {}],
		['anything deeper under a user', `${API}/users/42/followers`, {}],
		['an answer that is not a 200', `${API}/tracks`, { status: 404 }],
		['an answer read as JSON', `${API}/tracks`, { responseType: 'json' }],
		['a URL that is not one', 'https://[oops', {}]
	] satisfies [string, string, Answer][])('%s', async (_name, url, answer) => {
		await load();
		hello();
		request(url, answer);
		expect(heard).toEqual([]);
	});
});

describe('before and after the content script says hello', () => {
	it('holds what arrives first, hands it over in order on hello, then passes the rest on as it lands', async () => {
		await load();
		request(`${API}/tracks`, { body: '[1]' });
		request(`${API}/users/7`, { body: '{"id":7}' });
		expect(heard).toEqual([]);

		hello();
		expect(heard).toEqual(['/tracks\n[1]', '/users/7\n{"id":7}']);

		request(`${API}/resolve`, { body: '{}' });
		expect(heard).toEqual(['/tracks\n[1]', '/users/7\n{"id":7}', '/resolve\n{}']);
	});

	it('hands over nothing twice when a second hello comes', async () => {
		await load();
		request(`${API}/tracks`, { body: '[1]' });
		hello();
		hello();
		expect(heard).toEqual(['/tracks\n[1]']);
	});
});

describe('the page’s own request', () => {
	it('reaches the original open with exactly the arguments the page passed', async () => {
		await load();
		const xhr = new XMLHttpRequest();

		xhr.open('GET', `${API}/tracks`);
		xhr.open('POST', 'https://example.test/', true);
		xhr.open('GET', `${API}/users/7`, false, 'user', 'secret');
		// An `async` passed as undefined is not a missing one: `open` reads it as
		// false, so it has to arrive as an argument.
		// @ts-expect-error -- what untyped page code may pass; the DOM types want a boolean
		xhr.open('GET', `${API}/resolve`, undefined);

		expect(opened).toEqual([
			['GET', `${API}/tracks`],
			['POST', 'https://example.test/', true],
			['GET', `${API}/users/7`, false, 'user', 'secret'],
			['GET', `${API}/resolve`, undefined]
		]);
		expect(opened.map((args) => args.length)).toEqual([2, 3, 5, 3]);
	});
});
