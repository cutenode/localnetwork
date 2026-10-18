import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { USERINPUT } from '#lib/bug-report.ts';
import { RecordValidationError } from '../atproto/lexicons';
import { PublicRepo } from '../atproto/public-repo';
import { SCOPES } from '../atproto/oauth';
import { type Db, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { FakeNetwork } from '../testing/fakes';
import { AccountService } from './accounts';
import { browserLabel, BugReportService } from './bug-reports';

const OWNER = 'did:plc:localnetwork';

const USER = 'did:plc:reporter';

const SPACE = `at://${OWNER}/${USERINPUT.space}/3mspace`;

const CHROME_MAC =
	'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36';

const SAFARI_IPHONE =
	'Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1';

describe('browserLabel', () => {
	it('names the browser, its major version and the OS, leaving out the version browsers freeze', () => {
		expect(browserLabel(CHROME_MAC)).toBe('Chrome 141, macOS');
		expect(browserLabel(SAFARI_IPHONE)).toBe('Safari 26, iOS');
	});

	it('gives nothing for a client that names no browser', () => {
		expect(browserLabel('curl/8.1.2')).toBeUndefined();
		expect(browserLabel(null)).toBeUndefined();
	});
});

describe('BugReportService', () => {
	let db: Db;
	let net: FakeNetwork;
	let accounts: AccountService;

	const service = (space: string | undefined, sessions = [USER]) =>
		new BugReportService({
			space,
			repos: new PublicRepo({ resolve: async () => ({ pds: 'https://pds.test' }) }, silentLogger, net.publicFetch),
			userClient: async (did) => (sessions.includes(did) ? net.client(did) : null),
			accounts,
			logger: silentLogger
		});

	const signIn = (grantedScope: string) =>
		accounts.recordLogin({ did: USER, handle: 'reporter.test', pdsUrl: null, grantedScope, spacesCapable: false });

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		net = new FakeNetwork();
		accounts = new AccountService(db, silentLogger);
		await net.client(OWNER).putRecord({
			collection: USERINPUT.space,
			rkey: '3mspace',
			record: { $type: USERINPUT.space, name: 'Local Network', createdAt: '2026-10-17T00:00:00.000Z' }
		});
	});
	afterEach(() => db.destroy());

	it('is off until a space is configured', () => {
		expect(service(SPACE).enabled).toBe(true);
		expect(service(undefined).enabled).toBe(false);
	});

	it('knows whether a sign-in approved posting, which older ones never did', async () => {
		expect(await service(SPACE).canPost(USER)).toBe(false);
		await signIn('atproto');
		expect(await service(SPACE).canPost(USER)).toBe(false);
		await signIn(`atproto ${SCOPES.bugReport}`);
		expect(await service(SPACE).canPost(USER)).toBe(true);
	});

	it("posts into the space's current version from the reporter's own repo, and links to it", async () => {
		const url = await service(SPACE).post(USER, { title: 'Blank map', body: 'It never loads.' });

		const [stored] = net.records
			.entries()
			.filter(([key]) => key.startsWith(`${USER}|${USERINPUT.discussion}|`))
			.toArray();

		const space = net.records.get(`${OWNER}|${USERINPUT.space}|3mspace`)!;

		expect(url).toBe(`https://userinput.app/d/${USER}/${stored[1].uri.split('/').at(-1)}`);
		expect(stored[1].value).toMatchObject({
			$type: USERINPUT.discussion,
			space: { uri: space.uri, cid: space.cid },
			title: 'Blank map',
			body: 'It never loads.',
			tags: ['bug', 'web']
		});
	});

	it('posts nothing that breaks the lexicon', async () => {
		await expect(service(SPACE).post(USER, { title: '漢'.repeat(300), body: '' })).rejects.toBeInstanceOf(
			RecordValidationError
		);
		expect(net.records.keys().some((key) => key.startsWith(`${USER}|`))).toBe(false);
	});

	it('fails without a space to point at or a session to post with', async () => {
		await expect(service(undefined).post(USER, { title: 't', body: '' })).rejects.toThrow('no userinput.app space');
		await expect(service(`at://${OWNER}/${USERINPUT.space}/gone`).post(USER, { title: 't', body: '' })).rejects.toThrow(
			'is missing'
		);
		await expect(service(SPACE, []).post(USER, { title: 't', body: '' })).rejects.toThrow('no OAuth session');
		expect(net.records.keys().some((key) => key.startsWith(`${USER}|`))).toBe(false);
	});
});
