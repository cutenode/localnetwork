// Hands content.ts the track and user data SoundCloud's own web app loads.
//
// This runs in the page's main world, where content.ts cannot: an isolated
// world never sees the page's XMLHttpRequest. It wraps `open` so that a GET
// the page makes to its own API, for one of the answers content.ts reads, is
// listened to, and when SoundCloud answers, the text the page received is
// passed on. It makes no request of its own, changes none of the page's, and
// never touches what the page gets back.
//
// The page's API client uses XMLHttpRequest for all of this (checked on
// playlist, Likes, profile and track pages), so `fetch` is left alone.
//
// It starts at document_start, before the page's own scripts, and content.ts
// at document_idle, so whatever arrives before content.ts says it is listening
// is held and handed over then. After that each answer goes on as it lands.

(() => {
	const API_HOST = 'api-v2.soundcloud.com';
	const EVENT = 'tbx:page-data';
	const HELLO = 'tbx:page-data-hello';

	// The answers that carry tracks or users: a list of tracks by id, a URL
	// resolved, a playlist, a user, a user's likes, and the links on a profile.
	// Everything else the page asks for (comments, stats, adverts) is let by.
	const WANTED = /^\/(?:tracks|resolve|playlists\/\d+|users\/[^/]+(?:\/(?:likes|track_likes|web-profiles))?)$/;

	/** Held until content.ts says hello, then null. */
	let held: string[] | null = [];

	// One event per answer: its path, a newline, and the body as SoundCloud
	// sent it. A string crosses into the isolated world as it is.
	const tell = (path: string, body: string) => {
		const detail = `${path}\n${body}`;

		if (held) held.push(detail);
		else document.dispatchEvent(new CustomEvent(EVENT, { detail }));
	};

	document.addEventListener(HELLO, () => {
		const waiting = held ?? [];
		held = null;

		for (const detail of waiting) document.dispatchEvent(new CustomEvent(EVENT, { detail }));
	});

	// The path of a URL on the page's API that content.ts wants, else ''.
	const wantedPath = (url: string | URL) => {
		try {
			const target = new URL(url, location.href);

			return target.host === API_HOST && WANTED.test(target.pathname) ? target.pathname : '';
		} catch (_e) {
			return '';
		}
	};

	/** `open` as the page calls it: the method and the address, then whatever else it passed. */
	type Open = (this: XMLHttpRequest, method: string, url: string | URL, ...rest: unknown[]) => void;

	// The page's own `open`, taken off the prototype as a function in its own
	// right, to be called with the request each call is for.
	const open: Open = Object.getOwnPropertyDescriptor(XMLHttpRequest.prototype, 'open')!.value;

	// Whatever arguments the page passes go through as passed: `open` reads a
	// missing `async` as true and an undefined one as false, so their number
	// matters as much as their values.
	XMLHttpRequest.prototype.open = function (
		this: XMLHttpRequest,
		method: string,
		url: string | URL,
		...rest: unknown[]
	) {
		const path = String(method).toUpperCase() === 'GET' ? wantedPath(url) : '';

		if (path) {
			this.addEventListener('load', () => {
				// Only text the page reads as text: a 'json' or 'blob' response
				// would need converting, and SoundCloud's client asks for neither.
				if (this.status === 200 && (this.responseType === '' || this.responseType === 'text')) {
					tell(path, this.responseText);
				}
			});
		}

		open.call(this, method, url, ...rest);
	};
})();
