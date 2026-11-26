// `node tools/crates-io/cli.ts check`: whether each loclib crate's name on crates.io is ours. Pull
// requests that change a crate's manifest run it (ci.yml's locallibrary_crates paths), and setup runs
// it before it changes anything. It fails only when a spelling of a crate's name is on crates.io and
// the crate isn't ours there: under its exact name with an owner in OWNERS. A crate crates.io doesn't
// have yet gets a warning annotation instead, since loclib-release.yml can't publish a first version
// and setup has to, but a pull request shouldn't wait for setup to pass.
// Until setup has, loclib-release.yml publishes nothing (`new`, below).
//
// crates.io counts `loclib-aup` and `loclib_aup` as one name, and its API answers for either. The
// sparse index keeps each spelling under its own file, so the check asks it for each spelling, and the
// API only for the owners of the one it found under our exact name.
import type { Client } from './client.ts';

/**
 * The crates.io users the loclib crates belong to, by user id, which stays with the account when a
 * GitHub login changes hands: bnb (Tierney, https://crates.io/api/v1/users/bnb), who runs setup. A
 * name already on crates.io is ours only when one of these owns it.
 */
export const OWNERS = [{ id: 381657, login: 'bnb' }];

export const SETUP = 'node tools/crates-io/cli.ts setup';

export type Run = (
	command: string,
	args: string[],
	options?: { input?: string; env?: Record<string, string>; show?: boolean }
) => { status: number; stdout: string; stderr: string };

/** What the commands reach outside the process: commands, crates.io, the terminal, the environment and the output. */
export interface Ports {
	run: Run;
	crates: Client;
	ask: (prompt: string) => Promise<string>;
	env: NodeJS.ProcessEnv;
	log: (line: string) => void;
}

export type Verdict =
	| { crate: string; state: 'new' }
	| { crate: string; state: 'ours'; owners: string[] }
	| { crate: string; state: 'taken'; why: string };

/** The workspace's crates Cargo can publish: those without `publish = false`, which cargo metadata reports as `[]`. */
export const publishable = (run: Run): string[] => {
	const { status, stdout, stderr } = run('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked']);

	if (status !== 0) throw new Error(`cargo metadata failed: ${stderr}`);

	// SAFETY: cargo metadata's format version 1, whose `publish` is null when the manifest doesn't say.
	const { packages } = JSON.parse(stdout) as { packages: { name: string; publish: string[] | null }[] };

	return packages
		.filter(({ publish }) => publish?.length !== 0)
		.map(({ name }) => name)
		.toSorted();
};

/** Every spelling crates.io counts as `name`, lowercase, as the index keeps them: each `-` or `_` either way. */
export const spellings = (name: string): string[] => {
	const [first, ...rest] = name.toLowerCase().split(/[-_]/);

	return rest.reduce(
		(found, part) => found.flatMap((spelling) => [`${spelling}-${part}`, `${spelling}_${part}`]),
		[first]
	);
};

/** Whose `crate`'s name is on crates.io: nobody's, ours, or someone else's. */
const verdict = async (crate: string, crates: Client): Promise<Verdict> => {
	const exact = crate.toLowerCase();

	if (await crates.indexed(exact)) {
		const response = await crates.api(`/crates/${exact}/owners`);
		// SAFETY: the owners endpoint lists users and teams, each with its kind, id and login. Teams
		// have ids of their own, which can match a user's.
		const { users } = (await response.json()) as { users: { kind: string; id: number; login: string }[] };
		const owners = users.map(({ login }) => login);
		const ours = users.some(({ kind, id }) => kind === 'user' && OWNERS.some((owner) => owner.id === id));

		return ours
			? { crate, state: 'ours', owners }
			: { crate, state: 'taken', why: `crates.io has it, owned by ${owners.join(', ')}, none of them in OWNERS` };
	}

	// One at a time: the first one found settles it.
	for (const spelling of spellings(crate).filter((other) => other !== exact)) {
		if (await crates.indexed(spelling))
			return { crate, state: 'taken', why: `crates.io has ${spelling}, which it counts as the same name` };
	}

	return { crate, state: 'new' };
};

/** Each crate's verdict, one crate at a time, so the API's requests keep their gap. */
export const verdicts = async (names: string[], crates: Client): Promise<Verdict[]> => {
	const found: Verdict[] = [];

	for (const name of names) found.push(await verdict(name, crates));

	return found;
};

export const explain = (found: Verdict): string => {
	if (found.state === 'new')
		return `::warning title=Not on crates.io::${found.crate} isn't on crates.io yet: run \`${SETUP}\` to publish its first version`;

	if (found.state === 'ours') return `${found.crate}: ours (${found.owners.join(', ')})`;

	return `${found.crate}: taken, ${found.why}. Rename the crate`;
};

/**
 * `node tools/crates-io/cli.ts new`: the publishable crates the index doesn't have under their exact
 * name, one a line, and passes. loclib-release.yml publishes nothing while there are any, since
 * crates.io takes a crate's first version only from setup.
 */
export const unpublished = async ({ run, crates, log }: Ports): Promise<boolean> => {
	for (const crate of publishable(run)) if (!(await crates.indexed(crate.toLowerCase()))) log(crate);

	return true;
};

/** Passes unless a crate's name is taken: crates.io has it but not as ours. */
export const check = async ({ run, crates, log }: Ports): Promise<boolean> => {
	const found = await verdicts(publishable(run), crates);

	for (const each of found) log(explain(each));

	return found.every(({ state }) => state !== 'taken');
};
