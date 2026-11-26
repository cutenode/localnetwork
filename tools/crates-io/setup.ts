// The loclib crates on crates.io, set up the one way CI can't: crates.io takes a crate's first
// version only from a person with a token, and only then lets it trust a workflow for the rest.
//   node tools/crates-io/cli.ts status   says what setup would do: which crates crates.io doesn't
//                                        have yet, which names are someone else's, and whether the
//                                        crates-io environment exists. Changes nothing, needs no token.
//   node tools/crates-io/cli.ts setup    does it, and is safe to run again. It checks the names
//                                        (names.ts) and runs `cargo publish --dry-run` over the new
//                                        crates first, and changes nothing if either fails. Then it
//                                        creates the crates-io environment (deploys from main only),
//                                        publishes the crates crates.io doesn't have yet
//                                        (dependencies first), and for every crate adds
//                                        loclib-release.yml as its trusted publisher and turns on
//                                        "require trusted publishing", so tokens can't publish it
//                                        any more.
// setup reads a crates.io API token from CRATES_IO_TOKEN, or asks for it. Make one at
// https://crates.io/settings/tokens/new that expires soon, limited to crates matching `loclib-*`,
// with the scopes publish-new and trusted-publishing; delete it afterwards. Before it changes
// anything, setup checks the token belongs to an account in OWNERS and has trusted-publishing.
// crates.io has no way to ask whether it has publish-new, so cargo publish finds that out. The
// environment needs `gh` signed in as a repository admin.
import { type Client, HttpError } from './client.ts';
import { OWNERS, type Ports, type Run, SETUP, explain, publishable, verdicts } from './names.ts';

const OWNER = 'cutenode';

const NAME = 'localnetwork';

const WORKFLOW = 'loclib-release.yml';

const ENVIRONMENT = 'crates-io';

const ENVIRONMENT_API = `repos/${OWNER}/${NAME}/environments/${ENVIRONMENT}`;

/** `gh` with `args`, which has to succeed. */
const gh = (run: Run, args: string[], input?: string): string => {
	const { status, stdout, stderr } = run('gh', args, { input });

	if (status !== 0) throw new Error(`gh ${args.join(' ')} failed: ${stderr}`);

	return stdout;
};

/** Whether `crate` already trusts the workflow, in the environment. */
const trusts = async (crates: Client, crate: string, token: string): Promise<boolean> => {
	const response = await crates.api(`/trusted_publishing/github_configs?crate=${crate}`, { token });

	// SAFETY: crates.io's list of a crate's GitHub trusted publishers.
	const { github_configs: configs } = (await response.json()) as {
		github_configs: {
			repository_owner: string;
			repository_name: string;
			workflow_filename: string;
			environment: string | null;
		}[];
	};

	return configs.some(
		(config) =>
			config.repository_owner === OWNER &&
			config.repository_name === NAME &&
			config.workflow_filename === WORKFLOW &&
			config.environment === ENVIRONMENT
	);
};

/**
 * The account in OWNERS that `token` belongs to, when it has the trusted-publishing scope. crates.io's
 * /me takes no API token
 * (https://github.com/rust-lang/crates.io/blob/6898ff3b275ed7ce25544fb81ab31aeb90898810/src/controllers/user/me.rs#L38),
 * but a user's list of trusted publishers takes a scoped token with trusted-publishing, and only the
 * user's own; crates.io answers 403 to any other
 * (https://github.com/rust-lang/crates.io/blob/6898ff3b275ed7ce25544fb81ab31aeb90898810/src/controllers/trustpub/github_configs/list.rs#L107-L128).
 */
const tokenOwner = async (crates: Client, token: string) => {
	for (const owner of OWNERS) {
		try {
			await crates.api(`/trusted_publishing/github_configs?user_id=${owner.id}`, { token });

			return owner;
		} catch (error) {
			if (!(error instanceof HttpError && error.status === 403)) throw error;
		}
	}

	return undefined;
};

const crateList = (names: string[]) => names.join(' ') || 'none';

export const status = async ({ run, crates, log }: Ports): Promise<boolean> => {
	const found = await verdicts(publishable(run), crates);
	const taken = found.filter(({ state }) => state === 'taken');
	const named = (state: string) => crateList(found.filter((each) => each.state === state).map(({ crate }) => crate));

	log(`Not on crates.io yet: ${named('new')}`);
	log(`On crates.io: ${named('ours')}`);

	for (const each of taken) log(explain(each));

	const environment = run('gh', ['api', ENVIRONMENT_API]).status === 0;

	log(environment ? `GitHub has the ${ENVIRONMENT} environment` : `GitHub has no ${ENVIRONMENT} environment yet`);
	log(`Run \`${SETUP}\` to publish the new ones and have every crate trust ${WORKFLOW}.`);

	return taken.length === 0;
};

export const setup = async ({ run, crates, ask, env, log }: Ports): Promise<boolean> => {
	const all = publishable(run);
	const found = await verdicts(all, crates);
	const taken = found.filter(({ state }) => state === 'taken');

	if (taken.length > 0) {
		for (const each of taken) log(explain(each));

		log('Nothing changed: rename the crates whose names are taken, then run setup again.');

		return false;
	}

	const fresh = found.filter(({ state }) => state === 'new').map(({ crate }) => crate);

	// Cargo publishes the workspace's crates in dependency order and waits for each to be on the
	// index; the ones crates.io already has are released by CI instead.
	const publish = [
		'publish',
		'--workspace',
		'--locked',
		...all.flatMap((crate) => (fresh.includes(crate) ? [] : ['--exclude', crate]))
	];

	log(`Not on crates.io yet: ${crateList(fresh)}`);

	if (fresh.length > 0 && run('cargo', [...publish, '--dry-run'], { show: true }).status !== 0) {
		log('Nothing changed: the publish dry run failed.');

		return false;
	}

	const token = env.CRATES_IO_TOKEN || (await ask('crates.io token (publish-new, trusted-publishing, loclib-*): '));

	if (token === '') {
		log('Nothing changed: setup needs a crates.io token.');

		return false;
	}

	const owner = await tokenOwner(crates, token);

	if (owner === undefined) {
		const logins = OWNERS.map(({ login }) => login).join(' or ');

		log(`Nothing changed: the token has to be ${logins}'s, with the trusted-publishing scope.`);

		return false;
	}

	log(`Token: ${owner.login}'s, with trusted-publishing`);

	// Deployments to the environment only from main, so a branch can't publish.
	gh(
		run,
		['api', '-X', 'PUT', ENVIRONMENT_API, '--input', '-'],
		JSON.stringify({ deployment_branch_policy: { protected_branches: false, custom_branch_policies: true } })
	);

	const branches = gh(run, ['api', `${ENVIRONMENT_API}/deployment-branch-policies`, '--jq', '.branch_policies[].name']);

	if (!branches.split('\n').includes('main'))
		gh(run, [
			'api',
			'-X',
			'POST',
			`${ENVIRONMENT_API}/deployment-branch-policies`,
			'-f',
			'name=main',
			'-f',
			'type=branch'
		]);

	log(`${ENVIRONMENT} environment: deploys from main`);

	if (fresh.length > 0 && run('cargo', publish, { show: true, env: { CARGO_REGISTRY_TOKEN: token } }).status !== 0) {
		log('cargo publish failed: run setup again once it can publish.');

		return false;
	}

	for (const crate of all) {
		if (!(await trusts(crates, crate, token)))
			await crates.api('/trusted_publishing/github_configs', {
				method: 'POST',
				token,
				body: {
					github_config: {
						crate,
						repository_owner: OWNER,
						repository_name: NAME,
						workflow_filename: WORKFLOW,
						environment: ENVIRONMENT
					}
				}
			});

		await crates.api(`/crates/${crate}`, { method: 'PATCH', token, body: { crate: { trustpub_only: true } } });
		log(`${crate}: trusts ${WORKFLOW} in ${ENVIRONMENT}, tokens can't publish it`);
	}

	log('Delete the token at https://crates.io/settings/tokens');

	return true;
};
