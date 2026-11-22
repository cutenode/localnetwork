// What the root's verify scripts need beyond `pnpm install`, checked before their first step
// (run.ts), so a missing tool fails the run up front instead of after the long builds. Each comes
// with its install line from .github/CONTRIBUTING.md's Checks. Where CI pins a tool whose findings
// change between versions, a local version that differs gets a warning, and where a check skips
// without something, a note says what goes unchecked.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

type Command = [string, ...string[]];

interface Tool {
	name: string;
	/** A command that succeeds once the tool is ready. */
	probe: Command;
	/** Its install line, given the version CI pins. */
	install: (version: string) => string;
	/** Where CI pins its version: the file, and the pattern that finds the version in it. */
	pin?: [string, RegExp];
	/** Whether a version other than CI's gets a warning: its findings change between versions. */
	drift?: true;
}

interface Skip {
	/** Whether what the check needs is there. */
	ready: (env: NodeJS.ProcessEnv) => boolean;
	note: string;
	/** The step that skips without it, which the run then leaves out. */
	step?: string;
}

/** What stops a script before its first step, what its run should say, and the steps it leaves out. */
export interface Preflight {
	problems: string[];
	notes: string[];
	skipped: string[];
}

const REPO_CHECKS = '.github/workflows/repo-checks.yml';

const LOCALLIBRARY_CI = '.github/workflows/locallibrary-ci.yml';

const binstall = (crate: string) => (version: string) => `cargo binstall ${crate}@${version}`;

const brew = (formula: string) => () => `brew install ${formula}`;

const tools = new Map<string, Tool[]>([
	[
		'verify:rust',
		[
			{
				name: 'cargo-nextest',
				probe: ['cargo', 'nextest', '--version'],
				install: binstall('cargo-nextest'),
				pin: [LOCALLIBRARY_CI, /tool: cargo-nextest@([\d.]+)/]
			},
			{
				name: 'cargo-shear',
				probe: ['cargo', 'shear', '--version'],
				install: binstall('cargo-shear'),
				pin: [LOCALLIBRARY_CI, /tool: cargo-shear@([\d.]+)/]
			},
			{
				name: 'dist',
				probe: ['dist', '--version'],
				install: binstall('cargo-dist'),
				pin: ['dist-workspace.toml', /cargo-dist-version = "([\d.]+)"/]
			}
		]
	],
	[
		'verify:repo',
		[
			{
				name: 'Docker',
				probe: ['docker', 'info'],
				install: () => 'start Docker or OrbStack, which `pnpm policy` runs in'
			},
			{
				name: 'ShellCheck',
				probe: ['shellcheck', '--version'],
				install: brew('shellcheck'),
				pin: [REPO_CHECKS, /koalaman\/shellcheck:v([\d.]+)/],
				drift: true
			},
			{
				name: 'typos',
				probe: ['typos', '--version'],
				install: brew('typos-cli'),
				pin: [REPO_CHECKS, /crate-ci\/typos@\w+ # v([\d.]+)/],
				drift: true
			},
			{
				name: 'taplo',
				probe: ['taplo', '--version'],
				install: brew('taplo'),
				pin: [REPO_CHECKS, /tool: taplo@([\d.]+)/],
				drift: true
			},
			{ name: 'Go', probe: ['go', 'version'], install: brew('go') },
			{ name: 'lychee', probe: ['lychee', '--version'], install: brew('lychee') }
		]
	]
]);

// Started rather than looked up on the PATH, so a Windows .exe counts too.
const run = ([command, ...args]: Command, env: NodeJS.ProcessEnv) =>
	spawnSync(command, args, { env, encoding: 'utf8' });

const succeeds = (probe: Command, env: NodeJS.ProcessEnv): boolean => run(probe, env).status === 0;

/** The step in steps.json that lefthook skips without the Windows target and MinGW. */
export const WINDOWS_CLIPPY = 'pnpm exec lefthook run pre-push --job "clippy for Windows" --force';

const skips = new Map<string, Skip[]>([
	[
		'verify:rust',
		[
			{
				ready: (env) =>
					String(run(['rustup', 'target', 'list', '--installed'], env).stdout).includes('x86_64-pc-windows-gnu') &&
					succeeds(['x86_64-w64-mingw32-gcc', '--version'], env),
				note: 'clippy for Windows skipped, so nothing here checks the code DJs syncing drives on Windows run: rustup target add x86_64-pc-windows-gnu, brew install mingw-w64',
				step: WINDOWS_CLIPPY
			},
			{
				ready: (env) => succeeds(['sqlcipher', '-version'], env),
				note: 'the tests skip checking that SQLCipher itself reads what locallibrary encrypts: brew install sqlcipher'
			}
		]
	]
]);

// The version CI pins ('' for a tool it doesn't pin), or the problem when the pin's file no longer
// says it where this looks.
const pinned = ({ name, pin }: Tool, root: string): { version: string } | { problem: string } => {
	if (pin === undefined) return { version: '' };

	const [file, pattern] = pin;
	const version = pattern.exec(readFileSync(join(root, file), 'utf8'))?.[1];

	return version === undefined
		? { problem: `${file} no longer pins ${name} where tools/verify/tools.ts looks (${pattern})` }
		: { version };
};

// A tool that runs but isn't the version CI pins, where its findings change between versions.
const drift = ({ name, probe, pin, drift: warns }: Tool, version: string, env: NodeJS.ProcessEnv): string[] => {
	const local = warns ? (/\d+\.\d+\.\d+/.exec(run(probe, env).stdout)?.[0] ?? 'unknown') : version;

	return local === version ? [] : [`${name} ${local} isn't CI's ${version} (${pin?.[0]}), so its findings can differ`];
};

/** Checks what `script` needs, with `env`'s PATH, reading CI's pins from the files under `root`. */
export const preflight = (script: string, env: NodeJS.ProcessEnv, root = '.'): Preflight => {
	const problems: string[] = [];
	const notes: string[] = [];

	for (const tool of tools.get(script) ?? []) {
		const pin = pinned(tool, root);

		if ('problem' in pin) problems.push(pin.problem);
		else if (!succeeds(tool.probe, env)) problems.push(`${script} needs ${tool.name}: ${tool.install(pin.version)}`);
		else notes.push(...drift(tool, pin.version, env));
	}

	const unready = (skips.get(script) ?? []).filter(({ ready }) => !ready(env));

	return {
		problems,
		notes: [...notes, ...unready.map(({ note }) => note)],
		skipped: unready.flatMap(({ step }) => (step === undefined ? [] : [step]))
	};
};
