import { describe, expect, it } from 'vitest';
import { USER_AGENT, indexPath } from './client.ts';
import { api, index, metadata, world } from './fakes.ts';
import { OWNERS, check, publishable, spellings, unpublished } from './names.ts';

const METADATA = 'cargo metadata --no-deps --format-version 1 --locked';

const workspace = (...names: string[]) => ({
	[METADATA]: {
		stdout: metadata(...names.map((name): [string, null] => [name, null]), ['locallibrary-core', []])
	}
});

interface Owner {
	id: number;
	login: string;
	kind?: string;
}

const owners = (...users: Owner[]) => ({
	json: { users: users.map(({ kind = 'user', ...user }) => ({ kind, ...user })) }
});

const [BNB] = OWNERS;

const SOMEONE = { id: 1, login: 'someone' };

describe('indexPath', () => {
	it.each([
		['a', '1/a'],
		['ab', '2/ab'],
		['abc', '3/a/abc'],
		['loclib-model', 'lo/cl/loclib-model']
	])('keeps %s at %s', (name, path) => {
		expect(indexPath(name)).toBe(path);
	});
});

describe('spellings', () => {
	it('swaps every separator both ways, lowercase', () => {
		expect(spellings('Loclib-Aup')).toEqual(['loclib-aup', 'loclib_aup']);
		expect(spellings('a_b-c')).toEqual(['a-b-c', 'a-b_c', 'a_b-c', 'a_b_c']);
		expect(spellings('serde')).toEqual(['serde']);
	});
});

describe('publishable', () => {
	it('leaves out the crates with publish = false, in name order', () => {
		const { ports } = world(
			{},
			{ [METADATA]: { stdout: metadata(['loclib-b', null], ['locallibrary', []], ['loclib-a', ['crates-io']]) } }
		);

		expect(publishable(ports.run)).toEqual(['loclib-a', 'loclib-b']);
	});

	it("fails when cargo can't read the workspace", () => {
		const { ports } = world({}, { [METADATA]: { status: 101, stderr: 'no Cargo.toml' } });

		expect(() => publishable(ports.run)).toThrow('cargo metadata failed: no Cargo.toml');
	});
});

describe('check', () => {
	it('passes a crate crates.io has under its name, owned by an account in OWNERS', async () => {
		const { ports, lines, requests } = world(
			{ [index('loclib-a')]: {}, [api('GET', '/crates/loclib-a/owners')]: owners(SOMEONE, BNB) },
			workspace('loclib-a')
		);

		expect(await check(ports)).toBe(true);
		expect(lines).toEqual(['loclib-a: ours (someone, bnb)']);
		// The name was found, so the other spelling isn't asked for.
		expect(requests.map(({ route }) => route)).toEqual([index('loclib-a'), api('GET', '/crates/loclib-a/owners')]);
		expect(requests.every(({ userAgent }) => userAgent === USER_AGENT)).toBe(true);
	});

	it("warns about a crate crates.io doesn't have yet and passes, after asking the index for every spelling", async () => {
		const { ports, lines, requests } = world({}, workspace('loclib-a'));

		expect(await check(ports)).toBe(true);
		expect(lines).toEqual([
			"::warning title=Not on crates.io::loclib-a isn't on crates.io yet: run `node tools/crates-io/cli.ts setup` to publish its first version"
		]);
		expect(requests.map(({ route }) => route)).toEqual([index('loclib-a'), index('loclib_a')]);
	});

	it("fails a name crates.io has under another spelling, which its API wouldn't tell apart", async () => {
		const { ports, lines } = world(
			{ [index('loclib_a')]: {}, [api('GET', '/crates/loclib-a/owners')]: owners(BNB) },
			workspace('loclib-a')
		);

		expect(await check(ports)).toBe(false);
		expect(lines).toEqual([
			'loclib-a: taken, crates.io has loclib_a, which it counts as the same name. Rename the crate'
		]);
	});

	it('fails a name someone else owns', async () => {
		const { ports, lines } = world(
			{
				[index('loclib-a')]: {},
				[api('GET', '/crates/loclib-a/owners')]: owners(SOMEONE, { id: 2, login: 'github:x:y', kind: 'team' })
			},
			workspace('loclib-a')
		);

		expect(await check(ports)).toBe(false);
		expect(lines).toEqual([
			'loclib-a: taken, crates.io has it, owned by someone, github:x:y, none of them in OWNERS. Rename the crate'
		]);
	});

	it("goes by an owner's user id: an account that took the login isn't ours, nor a team with the id", async () => {
		const { ports, lines } = world(
			{
				[index('loclib-a')]: {},
				[api('GET', '/crates/loclib-a/owners')]: owners(
					{ id: 2, login: BNB.login },
					{ id: BNB.id, login: 'github:x:y', kind: 'team' }
				)
			},
			workspace('loclib-a')
		);

		expect(await check(ports)).toBe(false);
		expect(lines).toEqual([
			'loclib-a: taken, crates.io has it, owned by bnb, github:x:y, none of them in OWNERS. Rename the crate'
		]);
	});

	it('fails when the index answers anything but the file or a 404', async () => {
		const { ports } = world({ [index('loclib-a')]: { status: 503 } }, workspace('loclib-a'));

		await expect(check(ports)).rejects.toThrow('index.crates.io for loclib-a: HTTP 503');
	});

	it('fails when the API turns a request down', async () => {
		const { ports } = world(
			{ [index('loclib-a')]: {}, [api('GET', '/crates/loclib-a/owners')]: { status: 429 } },
			workspace('loclib-a')
		);

		await expect(check(ports)).rejects.toThrow('crates.io for GET /crates/loclib-a/owners: HTTP 429');
	});

	it('checks every publishable crate, and fails when one name is taken', async () => {
		const { ports, lines } = world(
			{
				[index('loclib-b')]: {},
				[api('GET', '/crates/loclib-b/owners')]: owners(BNB),
				[index('loclib_c')]: {}
			},
			workspace('loclib-c', 'loclib-b', 'loclib-a')
		);

		expect(await check(ports)).toBe(false);
		expect(lines).toEqual([
			expect.stringMatching(/^::warning title=Not on crates.io::loclib-a /),
			'loclib-b: ours (bnb)',
			expect.stringMatching(/^loclib-c: taken, crates.io has loclib_c/)
		]);
	});
});

describe('unpublished', () => {
	it("lists the crates the index doesn't have under their exact name, and passes", async () => {
		const { ports, lines, requests } = world(
			{ [index('loclib-b')]: {}, [index('loclib_a')]: {} },
			workspace('loclib-b', 'loclib-a')
		);

		expect(await unpublished(ports)).toBe(true);
		expect(lines).toEqual(['loclib-a']);
		// Only the index, and only each exact name.
		expect(requests.map(({ route }) => route)).toEqual([index('loclib-a'), index('loclib-b')]);
	});
});
