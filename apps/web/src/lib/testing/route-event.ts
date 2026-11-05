import type { Cookies } from '@sveltejs/kit';
import { type Logger, silentLogger } from '#lib/server/logger.ts';
import { FLAGS } from '#lib/server/flags/catalog.ts';
import type { Flags, Viewer } from '#lib/types.ts';

type MaybePromise<T> = T | Promise<T>;

/** What a test sets on a route's event. Anything left out gets a harmless default. */
export type RouteEventInit = {
	/** Path and query, resolved against https://localnetwork.test. */
	url?: string;
	params?: Record<string, string>;
	/** The services the route reads, usually createTestServices()'s, with stubs for the rest. */
	app?: object;
	viewer?: Viewer | null;
	/** Set by the server hook when the session's OAuth grant lacks scopes the app now asks for. */
	reauth?: boolean;
	sessionToken?: string;
	/** For a test that reads what the route logged; quiet otherwise. */
	log?: Logger;
	/** Sent as a POST's form body; leave out for a GET. */
	form?: Record<string, string | string[] | Blob>;
	/** A request of the test's own, in place of the one built from `url`, `form` and `headers`. */
	request?: Request;
	headers?: Record<string, string>;
	cookies?: Record<string, string>;
	/** Every flag is off unless a test sets it. */
	flags?: Partial<Flags>;
	/** What `parent()` resolves to in a load under a layout. */
	parent?: object;
	fetch?: typeof fetch;
	routeId?: string;
	clientAddress?: string;
};

/** A cookie jar standing in for SvelteKit's `cookies`, keeping what the route set and deleted for the test to read. */
export class CookieJar {
	readonly values: Map<string, string>;
	readonly options = new Map<string, Parameters<Cookies['set']>[2]>();
	readonly deleted = new Set<string>();

	constructor(initial: Record<string, string> = {}) {
		this.values = new Map(Object.entries(initial));
	}

	get(name: string): string | undefined {
		return this.values.get(name);
	}

	getAll(): { name: string; value: string }[] {
		return [...this.values].map(([name, value]) => ({ name, value }));
	}

	set(name: string, value: string, options: Parameters<Cookies['set']>[2]): void {
		this.values.set(name, value);
		this.options.set(name, options);
		this.deleted.delete(name);
	}

	delete(name: string): void {
		this.values.delete(name);
		this.deleted.add(name);
	}

	serialize(name: string, value: string): string {
		return `${name}=${value}`;
	}
}

/** The event a route's load, action or handler gets, and what the route did with its cookies and headers. */
export function routeEvent(init: RouteEventInit = {}) {
	const url = new URL(init.url ?? '/', 'https://localnetwork.test');
	const cookies = new CookieJar(init.cookies);
	const headers: Record<string, string> = {};

	const request =
		init.request ??
		new Request(url, {
			method: init.form ? 'POST' : 'GET',
			headers: init.headers,
			body: init.form ? formData(init.form) : undefined
		});

	const locals = {
		app: init.app ?? {},
		viewer: init.viewer ?? null,
		flags: { ...Object.fromEntries(FLAGS.map(({ key }) => [key, false])), ...init.flags },
		reauth: init.reauth,
		sessionToken: init.sessionToken,
		requestId: 'test-request',
		log: init.log ?? silentLogger
	};

	const event = {
		url,
		params: init.params ?? {},
		request,
		locals,
		cookies,
		route: { id: init.routeId ?? null },
		fetch: init.fetch ?? globalThis.fetch,
		getClientAddress: () => init.clientAddress ?? '203.0.113.7',
		setHeaders: (set: Record<string, string>) => Object.assign(headers, set),
		parent: async () => init.parent ?? {},
		depends: () => {},
		untrack: <T>(fn: () => T) => fn(),
		isDataRequest: false,
		isSubRequest: false
	};

	/** Calls a route's load, action or request handler with this event, as SvelteKit would. */
	const call = <R>(route: (event: never) => R): R =>
		// SAFETY: the event carries every field a route here reads, and the app the services its test gives.
		route(event as never);

	/** Calls a load with this event, and gives its data its route's type, as {@link loadRoute} does. */
	const loadData = async <Data>(load: (event: never) => MaybePromise<object | void>): Promise<Data> =>
		// SAFETY: SvelteKit generates the route's PageServerData or LayoutServerData from what this load returns.
		(await call(load)) as Data;

	return { event, cookies, headers, locals, call, loadData };
}

/**
 * Calls a route's load, action or request handler with an event built from `init`, the way SvelteKit would.
 * A redirect or `error()` comes back as the rejection, from a synchronous load too.
 */
export const callRoute = async <R>(route: (event: never) => R, init: RouteEventInit = {}): Promise<Awaited<R>> =>
	await routeEvent(init).call(route);

/**
 * A load's data, as its route's generated `PageServerData` or `LayoutServerData`. Every load here is declared as a
 * `PageServerLoad` or `LayoutServerLoad`, whose return type allows `void`, so the data has to take its route's own type.
 */
export const loadRoute = async <Data>(
	load: (event: never) => MaybePromise<object | void>,
	init: RouteEventInit = {}
): Promise<Data> => routeEvent(init).loadData<Data>(load);

const formData = (fields: NonNullable<RouteEventInit['form']>) => {
	const form = new FormData();

	for (const [name, value] of Object.entries(fields))
		for (const entry of Array.isArray(value) ? value : [value]) form.append(name, entry);

	return form;
};
