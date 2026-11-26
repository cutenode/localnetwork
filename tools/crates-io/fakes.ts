// Stand-ins for crates.io and the commands tools/crates-io runs, for its tests: nothing reaches the
// network or runs cargo or gh.
import { type Fetch, client, indexPath } from './client.ts';
import type { Ports, Run } from './names.ts';

/** The URL of `name`'s sparse index file. */
export const index = (name: string) => `GET https://index.crates.io/${indexPath(name)}`;

/** The URL of an API path, after its method. */
export const api = (method: string, path: string) => `${method} https://crates.io/api/v1${path}`;

/** cargo metadata's answer for a workspace of `packages`, each a name and its `publish`. */
export const metadata = (...packages: [string, string[] | null][]) =>
	JSON.stringify({ packages: packages.map(([name, publish]) => ({ name, publish })) });

/** A request the stand-in for crates.io got. */
interface Request {
	route: string;
	userAgent: string | null;
	authorization: string | null;
	body: unknown;
}

/** crates.io's answer to one route: 200 unless it says, with `json` as the body. */
interface Answer {
	status?: number;
	json?: object;
}

/**
 * crates.io answering each `method url` route, and 404 to the rest. Commands are answered by their
 * whole command line, with status 0 and no output for the rest.
 */
export const world = (
	routes: Record<string, Answer>,
	commands: Record<string, Partial<ReturnType<Run>>>,
	env: NodeJS.ProcessEnv = {},
	typed = ''
) => {
	const requests: Request[] = [];
	const ran: { line: string; input?: string; env?: Record<string, string>; show?: boolean }[] = [];
	const lines: string[] = [];

	const fetch: Fetch = async (url, init) => {
		const route = `${init.method ?? 'GET'} ${url}`;
		const headers = new Headers(init.headers);

		requests.push({
			route,
			userAgent: headers.get('User-Agent'),
			authorization: headers.get('Authorization'),
			body: init.body === undefined ? undefined : await new Response(init.body).json()
		});

		const { status = 200, json = {} } = routes[route] ?? { status: 404 };

		return Response.json(json, { status });
	};

	const run: Run = (command, args, options = {}) => {
		const line = [command, ...args].join(' ');

		ran.push({ line, ...options });

		const { status = 0, stdout = '', stderr = '' } = commands[line] ?? {};

		return { status, stdout, stderr };
	};

	const ports: Ports = {
		run,
		crates: client(fetch, 0),
		ask: async (prompt) => {
			lines.push(`asked: ${prompt}`);

			return typed;
		},
		env,
		log: (line) => lines.push(line)
	};

	return { ports, requests, ran, lines };
};
