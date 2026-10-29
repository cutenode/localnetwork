import { request } from 'node:http';
import { asDid, AtpAgent } from '@atproto/api';
import { mockNetworkUtilities, TestPds, TestPlc } from '@atproto/dev-env';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { z } from 'zod';
import { type HappyView, startHappyView } from '../../scripts/lib/happyview.ts';
import { USERINPUT } from '../../src/lib/bug-report.ts';
import { publishLexicons } from '../../src/lib/server/atproto/lexicons.ts';
import { grantsPublicRecords, grantsSpaces, type OptionalScope } from '../../src/lib/server/atproto/oauth.ts';
import { AgentRepoClient, type RepoClient } from '../../src/lib/server/atproto/repo-client.ts';
import { type App, createApp } from '../../src/lib/server/app.ts';
import { loadConfig } from '../../src/lib/server/config.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import type { Viewer } from '../../src/lib/types.ts';

export type TestUser = { did: string; handle: string; client: RepoClient; agent: AtpAgent };

/** What a test sign-in asks for besides the app's own sign-in: other scopes `requested` instead, or `extra` ones too. */
type ScopeRequest = { requested?: string; optional?: OptionalScope };

/**
 * A real atproto network: in-process, a PLC directory and an alpha PDS with
 * atproto spaces; in Docker, a HappyView indexing it (scripts/lib/happyview.ts);
 * and a Local Network app wired to all three. Users act through password
 * sessions, which the PDS lets do anything, except those listed in `oauth`,
 * who sign in through the app's OAuth client and are held to the scopes it
 * requests.
 */
export class TestNetwork {
	private constructor(
		readonly plc: TestPlc,
		readonly pds: TestPds,
		readonly happyView: HappyView,
		readonly rootDid: string,
		readonly users: Map<string, TestUser>,
		readonly app: App
	) {}

	static async create(names: string[], options: { admins?: string[]; oauth?: string[] } = {}): Promise<TestNetwork> {
		const plc = await TestPlc.create({});
		const pds = await TestPds.create({ didPlcUrl: plc.url, inviteRequired: false });
		mockNetworkUtilities([pds]);

		const rootPassword = 'root-password';
		const rootAgent = new AtpAgent({ service: pds.url });

		const root = await rootAgent.createAccount({
			handle: 'localnetwork.test',
			email: 'root@localnetwork.test',
			password: rootPassword
		});

		// As in the dev network, root is the lexicon authority, so the PDS can resolve the space
		// declarations it needs to grant the app's OAuth scopes.
		pds.ctx.cfg.lexicon.didAuthority = asDid(root.data.did);
		await publishLexicons(rootAgent);

		const { data: userinputSpace } = await rootAgent.com.atproto.repo.createRecord({
			repo: root.data.did,
			collection: USERINPUT.space,
			record: { $type: USERINPUT.space, name: 'Local Network', createdAt: new Date().toISOString() }
		});

		const users = new Map<string, TestUser>();

		for (const name of names) {
			const agent = new AtpAgent({ service: pds.url });
			const handle = `${name}.test`;
			await agent.createAccount({ handle, email: `${name}@example.test`, password: `${name}-password` });
			users.set(name, { did: agent.did!, handle, agent, client: new AgentRepoClient(agent.did!, agent) });
		}

		const happyView = await startHappyView({ plcUrl: plc.url, pdsUrl: pds.url });

		const config = loadConfig({
			NODE_ENV: 'test',
			PUBLIC_URL: 'http://127.0.0.1:5999',
			DATABASE_PATH: ':memory:',
			ROOT_IDENTIFIER: root.data.did,
			ROOT_PASSWORD: rootPassword,
			ROOT_SERVICE: pds.url,
			PLC_URL: plc.url,
			HANDLE_RESOLVER_URL: pds.url,
			HAPPYVIEW_URL: happyView.url,
			HAPPYVIEW_API_KEY: happyView.apiKey,
			ADMIN_DIDS: (options.admins ?? []).map((name) => users.get(name)!.did).join(','),
			SYNC_NOTIFY: 'false',
			GEOCODER: 'none',
			AUDIT_MAX_ATTEMPTS: '3',
			USERINPUT_SPACE: userinputSpace.uri
		});

		const app = await createApp({
			config,
			logger: silentLogger,
			userClient: async (did) => [...users.values()].find((user) => user.did === did)?.client ?? null,
			background: false
		});

		const net = new TestNetwork(plc, pds, happyView, root.data.did, users, app);

		for (const name of names) {
			if (options.oauth?.includes(name)) await net.signInWithOAuth(name);
			else await net.recordLogin(name, app.oauth.scopes.full);
		}

		await app.bootstrap();

		return net;
	}

	/**
	 * Signs a user in through the app's OAuth client and records the login like the callback does.
	 * `approve` narrows what they consent to, as the PDS lets people do. `requested` asks for other scopes
	 * than the app's first choice, like the fallback for a PDS without spaces, and `optional` asks for one
	 * of the optional scopes too, as opting in does. Returns the granted scope.
	 */
	async signInWithOAuth(name: string, approve?: string, scopes: ScopeRequest = {}): Promise<string> {
		const scope = await signInWithOAuth(this.app, this.user(name), `${name}-password`, approve, scopes);
		await this.recordLogin(name, scope);

		return scope;
	}

	private recordLogin(name: string, grantedScope: string): Promise<void> {
		const user = this.user(name);

		return this.app.accounts.recordLogin({
			did: user.did,
			handle: user.handle,
			pdsUrl: this.pds.url,
			grantedScope,
			spacesCapable: grantsSpaces(grantedScope, user.did),
			publicWrites: grantsPublicRecords(grantedScope, user.did)
		});
	}

	user(name: string): TestUser {
		const user = this.users.get(name);

		if (!user) throw new Error(`No test user ${name}`);

		return user;
	}

	/** The user as the web app sees them once signed in. */
	async viewer(name: string): Promise<Viewer> {
		const viewer = await this.app.viewer(this.user(name).did);

		if (!viewer) throw new Error(`${name} has to sign in again`);

		return viewer;
	}

	/** Lets the PDS finish background work (notifications), then syncs every space. */
	async settle(): Promise<void> {
		await this.app.audit.idle();
		await this.pds.processAll();
		await this.app.audit.processPending(500);
		await this.app.syncer.syncAll();
	}

	async close(): Promise<void> {
		await this.app.audit.idle();
		await this.app.stop();
		await this.happyView.stop();
		await this.pds.close();
		await this.plc.close();
	}
}

/**
 * Signs a user in through the app's OAuth client by calling the PDS's sign-in and consent API the
 * way its authorization page does in a browser, then swaps their client for the OAuth session.
 * Returns the granted scope.
 */
async function signInWithOAuth(
	app: App,
	user: TestUser,
	password: string,
	approve?: string,
	{ requested, optional }: ScopeRequest = {}
): Promise<string> {
	const page = requested
		? await app.oauth.client.authorize(user.handle, { scope: requested, state: '{}' })
		: await app.oauth.authorize(user.handle, '{}', { account: await app.accounts.get(user.did), optional });

	// The page's scripts normally pick up a CSRF cookie; the API only checks it matches the header.
	const csrf = 'c'.repeat(24);
	const cookies = new Map([['csrf-token', csrf]]);

	// node:http rather than fetch, which overrides the sec-fetch-* headers the PDS checks.
	const send = (url: URL, headers: Record<string, string>, body?: JsonObject) =>
		new Promise<string>((resolve, reject) => {
			const cookie = [...cookies].map((pair) => pair.join('=')).join('; ');

			const req = request(url, { method: body ? 'POST' : 'GET', headers: { ...headers, cookie } }, (res) => {
				for (const header of res.headers['set-cookie'] ?? []) {
					const pair = header.split(';')[0];
					cookies.set(pair.slice(0, pair.indexOf('=')), pair.slice(pair.indexOf('=') + 1));
				}

				let text = '';
				res.setEncoding('utf8');
				res.on('data', (chunk: string) => (text += chunk));
				// A redirect here is the PDS sending the user back to the app with an error.
				res.on('end', () =>
					res.statusCode! < 300
						? resolve(text)
						: reject(new Error(`${url.pathname}: ${res.statusCode} ${res.headers.location ?? text}`))
				);
			});

			req.on('error', reject);
			req.end(body ? JSON.stringify(body) : undefined);
		});

	await send(page, { 'sec-fetch-mode': 'navigate', 'sec-fetch-dest': 'document', 'sec-fetch-site': 'cross-site' });

	const api = async (endpoint: string, body: JsonObject): Promise<JsonValue> => {
		const response = await send(
			new URL(`/@atproto/oauth-provider/~api${endpoint}`, page),
			{
				'content-type': 'application/json',
				origin: page.origin,
				referer: page.href,
				'sec-fetch-mode': 'same-origin',
				'sec-fetch-site': 'same-origin',
				'x-csrf-token': csrf
			},
			body
		);

		return JSON.parse(response);
	};

	await api('/sign-in', { locale: 'en', username: user.handle, password, remember: true });
	// Consent answers with the redirect back to the app's callback.
	const { url } = z.object({ url: z.string() }).parse(await api('/consent', { did: user.did, scope: approve }));
	const { scope } = await app.oauth.callback(new URL(url).searchParams);
	user.client = (await app.oauth.repoClient(user.did))!;

	return scope;
}
