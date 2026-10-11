// A fetch stub for SoundCloud's API: routes each request through `handler`
// (given the parsed URL, the init, and the 1-based call number) and records
// every call. Unrouted requests answer 404.
import { vi } from 'vitest';

/** A Response, as far as the content script reads one. */
export interface FakeResponse<Body = unknown> {
	ok: boolean;
	status: number;
	json: () => Promise<Body>;
}

/**
 * Routes one request: an answer, or nothing for the 404 an unrouted one gets. A promise is the
 * answer as it is, so one that never settles is a request that never comes back.
 */
export type Route = (
	url: URL,
	init: RequestInit | undefined,
	call: number
) => FakeResponse | Promise<FakeResponse> | undefined;

export const jsonResponse = <Body>(body: Body, status = 200) => ({
	ok: status >= 200 && status < 300,
	status,
	json: async () => structuredClone(body)
});

export function mockFetch(handler: Route = () => undefined) {
	const calls: { url: URL; init: RequestInit | undefined }[] = [];

	const fn = vi.fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<FakeResponse>>(async (input, init) => {
		const url = new URL(input instanceof Request ? input.url : input);
		calls.push({ url, init });

		return handler(url, init, calls.length) ?? jsonResponse({ error: 'not found' }, 404);
	});

	vi.stubGlobal('fetch', fn);

	return { fn, calls, paths: () => calls.map((c) => c.url.pathname + c.url.search) };
}

// A likes listing page as /users/<id>/likes returns it.
export const likesListing = <Track>(tracks: (Track | null)[], nextHref: string | null = null) => ({
	collection: tracks.map((track) => (track ? { track, created_at: '2026-01-01T00:00:00Z' } : null)),
	next_href: nextHref
});
