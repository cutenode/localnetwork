// What conftest runs the policies in tools/policy/ over: the repository's config files, and inputs
// made from the rest of it, which conftest can't read itself. Paths are from the repository root,
// which is where `pnpm policy` runs.
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { createRailwayContext, project } from 'railway/iac';
import railway from '../../.railway/railway.ts';
import { repoConfig } from '../labeler/labeler.ts';
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
		':(glob).github/ISSUE_TEMPLATE/*.yml',
		':(glob).github/rulesets/*.json',
		':(glob)lexicons/**/*.json'
	);

/** The lines of a Markdown file's bash and sh code blocks, trimmed, without the empty ones. */
export const shellLines = (markdown: string): string[] =>
	[...markdown.matchAll(/^```(?:bash|sh)\n([\s\S]*?)^```/gm)].flatMap(([, block = '']) =>
		block.split('\n').flatMap((line) => line.trim() || [])
	);

/** What crateRoots reads of `cargo metadata --no-deps --format-version 1`. */
export interface CargoMetadata {
	workspace_root: string;
	packages: { manifest_path: string; targets: { kind: string[]; src_path: string }[] }[];
}

// The kinds cargo metadata gives a package's [lib] target, one for each crate type it builds.
const LIBRARY = new Set(['lib', 'rlib', 'dylib', 'cdylib', 'staticlib', 'proc-macro']);

// The crate's own attributes: each `#![...]` whose parent is the root's file itself, as ast-grep
// parses Rust. Rust takes those only before the first item. One inside an inline module is that
// module's, and isn't matched.
const CRATE_ATTRIBUTES = [
	'id: crate-attributes',
	'language: rust',
	'rule:',
	'  kind: inner_attribute_item',
	'  inside:',
	'    kind: source_file'
].join('\n');

/**
 * Each workspace library's root, where its [lib] says (src/lib.rs unless it says otherwise), and
 * the crate attributes there, by manifest. A package without a library isn't listed.
 */
export const crateRoots = ({
	workspace_root,
	packages
}: CargoMetadata): Record<string, { root: string; attributes: string[] }> => {
	const libraries = packages.flatMap(({ manifest_path, targets }) =>
		targets
			.filter(({ kind }) => kind.some((each) => LIBRARY.has(each)))
			.map(({ src_path }) => ({ manifest_path, src_path }))
	);

	const { stdout } = spawnSync(
		resolve('node_modules/.bin/ast-grep'),
		['scan', '--inline-rules', CRATE_ATTRIBUTES, '--json=compact', ...libraries.map(({ src_path }) => src_path)],
		{ encoding: 'utf8' }
	);

	// SAFETY: ast-grep's JSON report, a match for each attribute, naming its file as it was given.
	const matches: { file: string; text: string }[] = JSON.parse(stdout);

	return Object.fromEntries(
		libraries.map(({ manifest_path, src_path }) => [
			relative(workspace_root, manifest_path),
			{
				root: relative(workspace_root, src_path),
				attributes: matches.flatMap(({ file, text }) => (file === src_path ? [text] : []))
			}
		])
	);
};

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
		'harden.json': { harden },
		// The commands CONTRIBUTING gives, for the policy that they cover what CI runs on a pull
		// request (contributing.rego).
		'contributing.json': { commands: shellLines(readFileSync('.github/CONTRIBUTING.md', 'utf8')) },
		// The config pr-labels.yml gives actions/labeler, which the repository doesn't keep.
		'labeler.json': JSON.parse(repoConfig()),
		// Each library's root and its crate attributes, for the policy that a crate going to crates.io
		// warns of public items without docs (cargo.rego).
		'crate-roots.json': {
			roots: crateRoots(
				JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { encoding: 'utf8' }))
			)
		}
	};

	for (const [name, value] of Object.entries(inputs)) {
		writeFileSync(join(scratch, name), JSON.stringify(value));
	}

	// Under a name conftest parses as TOML.
	copyFileSync('Cargo.lock', join(scratch, 'Cargo.lock.toml'));

	return [...Object.keys(inputs), 'Cargo.lock.toml'];
};
