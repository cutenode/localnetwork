// `pnpm verify:rust --package <crate>`: one crate's format, clippy and tests, with the workspace's
// cargo shear and `pnpm policy`, so writing a plugin doesn't mean building the CLI and the player.
// It ends by listing what the full `pnpm verify:rust` adds.
import { type Verdict, steps as scripts, verify } from './run.ts';
import { CRATE } from './tools.ts';

/** The steps that check `crate` alone. */
export const crateSteps = (crate: string): string[] => [
	`cargo fmt -p ${crate} --check`,
	`cargo clippy -p ${crate} --all-targets --locked -- -D warnings`,
	`cargo nextest run -p ${crate} --locked`,
	'cargo shear',
	'pnpm policy'
];

/** Runs `steps` (`crate`'s by default), then lists the full run's steps that `crate`'s leave out. */
export const verifyCrate = (crate: string, env = process.env, steps = crateSteps(crate)): Verdict => {
	// The name goes into shell commands.
	if (!/^[\w-]+$/.test(crate)) return { lines: [`${crate} isn't a crate name`], ok: false };

	const { lines, ok } = verify(CRATE, new Map([[CRATE, steps]]), env);

	const left = [...scripts]
		.flatMap(([script, list]) => (script === 'verify:rust' ? list : []))
		.filter((step) => !crateSteps(crate).includes(step));

	return { lines: [...lines, '', 'The full `pnpm verify:rust` also runs:', ...left.map((step) => `  ${step}`)], ok };
};
