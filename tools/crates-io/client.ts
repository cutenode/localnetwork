// What tools/crates-io asks crates.io: its sparse index (index.crates.io), which a CDN serves, and
// its API. Every request names the repository in its User-Agent, and API requests go out a second
// apart, as crates.io's data access policy asks (https://crates.io/data-access).

/** The global fetch, or the tests' stand-in. */
export type Fetch = (url: string, init: RequestInit) => Promise<Response>;

/** crates.io's answer when it isn't a 2xx, with its status. */
export class HttpError extends Error {
	status: number;

	constructor(what: string, status: number, body: string) {
		super(`${what}: HTTP ${status} ${body}`);
		this.status = status;
	}
}

export const USER_AGENT = 'localnetwork tools/crates-io (https://github.com/cutenode/localnetwork)';

const INDEX = 'https://index.crates.io';

const API = 'https://crates.io/api/v1';

/** The sparse index's file for `name`: by its length up to 3, then by its first two pairs of letters. */
export const indexPath = (name: string): string => {
	if (name.length <= 2) return `${name.length}/${name}`;

	if (name.length === 3) return `3/${name[0]}/${name}`;

	return `${name.slice(0, 2)}/${name.slice(2, 4)}/${name}`;
};

interface Call {
	method?: string;
	token?: string;
	body?: unknown;
}

/** crates.io through `fetch`, with `gap` milliseconds before each API request. */
export const client = (fetch: Fetch, gap = 1000) => {
	const request = (url: string, init: { method?: string; headers?: Record<string, string>; body?: string } = {}) =>
		fetch(url, { ...init, headers: { 'User-Agent': USER_AGENT, ...init.headers } });

	const failure = async (what: string, response: Response) =>
		new HttpError(what, response.status, await response.text());

	return {
		/** Whether the index has a file under exactly `name`, which has to be lowercase: the index doesn't fold `-` and `_`. */
		indexed: async (name: string): Promise<boolean> => {
			const response = await request(`${INDEX}/${indexPath(name)}`);

			if (response.status === 404) return false;

			if (!response.ok) throw await failure(`index.crates.io for ${name}`, response);

			await response.text();

			return true;
		},
		/** An API request with the token, if any. Anything but a 2xx fails. */
		api: async (path: string, { method = 'GET', token, body }: Call = {}): Promise<Response> => {
			await new Promise((resolve) => setTimeout(resolve, gap));

			const headers: Record<string, string> = {};

			if (token !== undefined) headers.Authorization = token;

			if (body !== undefined) headers['Content-Type'] = 'application/json';

			const response = await request(`${API}${path}`, {
				method,
				headers,
				body: body === undefined ? undefined : JSON.stringify(body)
			});

			if (!response.ok) throw await failure(`crates.io for ${method} ${path}`, response);

			return response;
		}
	};
};

export type Client = ReturnType<typeof client>;
