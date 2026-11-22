// What conftest runs the policies in tools/policy/ over: the repository's config files, and inputs
// made from the rest of it, which conftest can't read itself. Paths are from the repository root,
// which is where `pnpm policy` runs.
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { createRailwayContext, project } from 'railway/iac';
import railway from '../../.railway/railway.ts';
import { reads } from '../reads/reads.ts';

// Tracked files and new ones not yet added (but not ignored ones), so a new app is checked too.
const list = (...pathspecs: string[]): string[] =>
	execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '--', ...pathspecs], {
		encoding: 'utf8'
	})
		.split('\n')
		.filter(Boolean);

/** The config files the policies read. */
export const configFiles = (): string[] =>
	list(
		'package.json',
		'pnpm-workspace.yaml',
		'Cargo.toml',
		'lefthook.yml',
		'tools/verify/steps.json',
		'deny.toml',
		'about.toml',
		'dist-workspace.toml',
		'.cargo/config.toml',
		':(glob)apps/*/package.json',
		':(glob)apps/*/Cargo.toml',
		':(glob)apps/*/Dockerfile',
		':(glob)packages/*/package.json',
		':(glob)crates/*/Cargo.toml',
		':(glob)plugins/*/Cargo.toml',
		':(glob)vendor/*/Cargo.toml',
		':(glob).github/workflows/*.yml',
		':(glob).github/actions/*/action.yml',
		'.github/dependabot.yml',
		'.github/labels.yml',
		'.github/labeler.yml',
		':(glob).github/ISSUE_TEMPLATE/*.yml',
		':(glob).github/rulesets/*.json',
		':(glob)lexicons/**/*.json'
	);

// The projects whose package manager's supply-chain settings harden checks (pnpm.rego).
const hardened = [
	{ manager: 'pnpm', folder: '.' },
	{ manager: 'npm', folder: 'apps/locallibrary/bombadil' }
];

/** Writes the inputs made from the repository to `scratch`, and returns their file names there. */
export const writeInputs = async (scratch: string): Promise<string[]> => {
	const files = list();

	// harden exits 1 whenever a check is unmet, which the runner's always is, so its report is read
	// whatever the status. An empty one fails to parse, and so stops the run. pnpm.rego reads it.
	const harden = hardened.map(({ manager, folder }) => {
		const { stdout } = spawnSync(
			resolve('node_modules/.bin/harden'),
			['check', '-p', manager, '-l', 'strict', '--json'],
			{
				cwd: folder,
				encoding: 'utf8'
			}
		);

		// Passed on to pnpm.rego as it is, which checks what's in it.
		const report: object = JSON.parse(stdout);

		return { manager, folder, report };
	});

	const inputs = {
		// Every file, for the policy that each one gets a label.
		'tracked-files.json': { files },
		// What each project's code reads from outside its own folder, for the policy that the
		// project's paths filter in ci.yml has it.
		'reads.json': { reads: reads(files, (file) => readFileSync(file, 'utf8')) },
		// The project as the railway SDK evaluates .railway/railway.ts. `pnpm check:tools` type-checks
		// the file against the SDK.
		'railway-project.json': await railway(createRailwayContext(), project),
		'harden.json': { harden }
	};

	for (const [name, value] of Object.entries(inputs)) {
		writeFileSync(join(scratch, name), JSON.stringify(value));
	}

	// Under a name conftest parses as TOML.
	copyFileSync('Cargo.lock', join(scratch, 'Cargo.lock.toml'));

	return [...Object.keys(inputs), 'Cargo.lock.toml'];
};
