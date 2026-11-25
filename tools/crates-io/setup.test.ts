import { describe, expect, it } from 'vitest';
import { api, index, metadata, world } from './fakes.ts';
import { OWNERS } from './names.ts';
import { setup, status } from './setup.ts';

const METADATA = 'cargo metadata --no-deps --format-version 1 --locked';

const [BNB] = OWNERS;

const WHOSE = api('GET', `/trusted_publishing/github_configs?user_id=${BNB.id}`);

const ENVIRONMENT = 'repos/cutenode/localnetwork/environments/crates-io';

const POLICIES = `gh api ${ENVIRONMENT}/deployment-branch-policies --jq .branch_policies[].name`;

const PUBLISH = 'cargo publish --workspace --locked --exclude loclib-a';

const trusted = {
	repository_owner: 'cutenode',
	repository_name: 'localnetwork',
	workflow_filename: 'loclib-release.yml',
	environment: 'crates-io'
};

// loclib-a is on crates.io and ours, loclib-b isn't yet, and locallibrary-core is never published.
const crates = {
	[index('loclib-a')]: {},
	[api('GET', '/crates/loclib-a/owners')]: { json: { users: [{ kind: 'user', ...BNB }] } },
	// The token is bnb's, with trusted-publishing.
	[api('GET', `/trusted_publishing/github_configs?user_id=${BNB.id}`)]: { json: { github_configs: [] } },
	[api('GET', '/trusted_publishing/github_configs?crate=loclib-a')]: { json: { github_configs: [trusted] } },
	[api('GET', '/trusted_publishing/github_configs?crate=loclib-b')]: {
		json: { github_configs: [{ ...trusted, environment: null }] }
	},
	[api('POST', '/trusted_publishing/github_configs')]: {},
	[api('PATCH', '/crates/loclib-a')]: {},
	[api('PATCH', '/crates/loclib-b')]: {}
};

const commands = {
	[METADATA]: { stdout: metadata(['loclib-a', null], ['loclib-b', null], ['locallibrary-core', []]) }
};

const writes = (requests: { route: string }[]) =>
	requests.flatMap(({ route }) => (route.startsWith('GET ') ? [] : [route]));

describe('status', () => {
	it("says what crates.io has, and that GitHub hasn't the environment", async () => {
		const { ports, lines } = world(crates, { ...commands, [`gh api ${ENVIRONMENT}`]: { status: 1 } });

		expect(await status(ports)).toBe(true);
		expect(lines).toEqual([
			'Not on crates.io yet: loclib-b',
			'On crates.io: loclib-a',
			'GitHub has no crates-io environment yet',
			'Run `node tools/crates-io/cli.ts setup` to publish the new ones and have every crate trust loclib-release.yml.'
		]);
	});

	it('says when GitHub has the environment, and fails on a name that is taken', async () => {
		const { ports, lines } = world({ ...crates, [index('loclib_b')]: {} }, commands);

		expect(await status(ports)).toBe(false);
		expect(lines).toEqual([
			'Not on crates.io yet: none',
			'On crates.io: loclib-a',
			'loclib-b: taken, crates.io has loclib_b, which it counts as the same name. Rename the crate',
			'GitHub has the crates-io environment',
			expect.stringMatching(/^Run /)
		]);
	});
});

describe('setup', () => {
	it('publishes the new crates, then has every crate trust the workflow and only the workflow', async () => {
		const { ports, lines, ran, requests } = world(crates, commands, { CRATES_IO_TOKEN: 'secret' });

		expect(await setup(ports)).toBe(true);
		expect(ran.map(({ line }) => line)).toEqual([
			METADATA,
			`${PUBLISH} --dry-run`,
			`gh api -X PUT ${ENVIRONMENT} --input -`,
			POLICIES,
			`gh api -X POST ${ENVIRONMENT}/deployment-branch-policies -f name=main -f type=branch`,
			PUBLISH
		]);
		expect(ran[2]?.input).toBe(
			JSON.stringify({ deployment_branch_policy: { protected_branches: false, custom_branch_policies: true } })
		);
		expect(ran.at(-1)?.env).toEqual({ CARGO_REGISTRY_TOKEN: 'secret' });
		// loclib-a trusts it already; loclib-b's publisher has no environment, so it gets one that has.
		expect(writes(requests)).toEqual([
			api('PATCH', '/crates/loclib-a'),
			api('POST', '/trusted_publishing/github_configs'),
			api('PATCH', '/crates/loclib-b')
		]);
		expect(requests.find(({ route }) => route.startsWith('POST'))?.body).toEqual({
			github_config: { crate: 'loclib-b', ...trusted }
		});
		expect(requests.find(({ route }) => route.startsWith('PATCH'))?.body).toEqual({ crate: { trustpub_only: true } });
		// Only the owners lookup goes without the token.
		expect(requests.filter(({ route }) => !route.includes('index')).map(({ authorization }) => authorization)).toEqual([
			null,
			...Array.from({ length: 6 }, () => 'secret')
		]);
		expect(lines).toEqual([
			'Not on crates.io yet: loclib-b',
			"Token: bnb's, with trusted-publishing",
			'crates-io environment: deploys from main',
			"loclib-a: trusts loclib-release.yml in crates-io, tokens can't publish it",
			"loclib-b: trusts loclib-release.yml in crates-io, tokens can't publish it",
			'Delete the token at https://crates.io/settings/tokens'
		]);
	});

	it('asks for the token, and publishes nothing when crates.io has every crate', async () => {
		const { ports, lines, ran } = world(
			{
				...crates,
				[index('loclib-b')]: {},
				[api('GET', '/crates/loclib-b/owners')]: { json: { users: [{ kind: 'user', ...BNB }] } }
			},
			{ ...commands, [POLICIES]: { stdout: 'release\nmain\n' } },
			{},
			'typed'
		);

		expect(await setup(ports)).toBe(true);
		expect(lines.slice(0, 2)).toEqual([
			'Not on crates.io yet: none',
			'asked: crates.io token (publish-new, trusted-publishing, loclib-*): '
		]);
		expect(ran.map(({ line }) => line)).toEqual([METADATA, `gh api -X PUT ${ENVIRONMENT} --input -`, POLICIES]);
	});

	it('changes nothing when a name is taken', async () => {
		const { ports, lines, ran, requests } = world(
			{ ...crates, [api('GET', '/crates/loclib-a/owners')]: { json: { users: [{ login: 'someone' }] } } },
			commands,
			{ CRATES_IO_TOKEN: 'secret' }
		);

		expect(await setup(ports)).toBe(false);
		expect(ran.map(({ line }) => line)).toEqual([METADATA]);
		expect(writes(requests)).toEqual([]);
		expect(lines).toEqual([
			'loclib-a: taken, crates.io has it, owned by someone, none of them in OWNERS. Rename the crate',
			'Nothing changed: rename the crates whose names are taken, then run setup again.'
		]);
	});

	it('changes nothing when the publish dry run fails', async () => {
		const { ports, lines, ran, requests } = world(
			crates,
			{ ...commands, [`${PUBLISH} --dry-run`]: { status: 101 } },
			{ CRATES_IO_TOKEN: 'secret' }
		);

		expect(await setup(ports)).toBe(false);
		expect(ran.map(({ line }) => line)).toEqual([METADATA, `${PUBLISH} --dry-run`]);
		expect(writes(requests)).toEqual([]);
		expect(lines.at(-1)).toBe('Nothing changed: the publish dry run failed.');
	});

	it('changes nothing without a token', async () => {
		const { ports, lines, ran } = world(crates, commands);

		expect(await setup(ports)).toBe(false);
		expect(ran.map(({ line }) => line)).toEqual([METADATA, `${PUBLISH} --dry-run`]);
		expect(lines.at(-1)).toBe('Nothing changed: setup needs a crates.io token.');
	});

	it("changes nothing when the token isn't an owner's with trusted-publishing", async () => {
		const { ports, lines, ran, requests } = world({ ...crates, [WHOSE]: { status: 403 } }, commands, {
			CRATES_IO_TOKEN: 'secret'
		});

		expect(await setup(ports)).toBe(false);
		expect(ran.map(({ line }) => line)).toEqual([METADATA, `${PUBLISH} --dry-run`]);
		expect(writes(requests)).toEqual([]);
		expect(requests.at(-1)?.authorization).toBe('secret');
		expect(lines.at(-1)).toBe("Nothing changed: the token has to be bnb's, with the trusted-publishing scope.");
	});

	it("stops when crates.io can't say whose the token is", async () => {
		const { ports, ran } = world({ ...crates, [WHOSE]: { status: 500 } }, commands, { CRATES_IO_TOKEN: 'secret' });

		await expect(setup(ports)).rejects.toThrow(
			`crates.io for GET /trusted_publishing/github_configs?user_id=${BNB.id}: HTTP 500`
		);
		expect(ran.map(({ line }) => line)).toEqual([METADATA, `${PUBLISH} --dry-run`]);
	});

	it('stops before the trusted publishers when cargo publish fails', async () => {
		const { ports, lines, requests } = world(
			crates,
			{ ...commands, [PUBLISH]: { status: 101 } },
			{ CRATES_IO_TOKEN: 's' }
		);

		expect(await setup(ports)).toBe(false);
		expect(writes(requests)).toEqual([]);
		expect(lines.at(-1)).toBe('cargo publish failed: run setup again once it can publish.');
	});

	it("stops when gh can't set up the environment", async () => {
		const { ports, requests } = world(
			crates,
			{ ...commands, [`gh api -X PUT ${ENVIRONMENT} --input -`]: { status: 1, stderr: 'HTTP 403' } },
			{ CRATES_IO_TOKEN: 's' }
		);

		await expect(setup(ports)).rejects.toThrow(`gh api -X PUT ${ENVIRONMENT} --input - failed: HTTP 403`);
		expect(writes(requests)).toEqual([]);
	});
});
