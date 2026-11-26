import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { mergeMain } from './merge-main.ts';

let repo: string;

const git = (...args: string[]) => execFileSync('git', args, { cwd: repo, encoding: 'utf8' });

const write = (path: string, text: string) => {
	mkdirSync(dirname(join(repo, path)), { recursive: true });
	writeFileSync(join(repo, path), text);
};

const read = (path: string) => readFileSync(join(repo, path), 'utf8');

const commit = (message: string) => {
	git('add', '-A');
	git('commit', '-q', '-m', message);
};

const run = () => {
	const log: string[] = [];

	return { ok: mergeMain(repo, (line) => log.push(line)), log };
};

const unmerged = () => [...new Set(git('diff', '--name-only', '--diff-filter=U').split('\n').filter(Boolean))];

// Everything a run could change: the index, the worktree and what's untracked.
const state = () => [git('ls-files', '--stage'), git('diff'), git('status', '--porcelain')].join('\0');

const readme = (features: string, setup = 'Run it.') =>
	`# Web\n\nIntro.\n\n## Features\n\n${features}\n\n## Setup\n\n${setup}\n`;

const bullets = (tools: string, bugs: string, flags: string, ...more: string[]) =>
	[`### Tools\n\n- ${tools}`, `### Bug reports\n\n${bugs}`, `### Flags\n\n${flags}`, ...more].join('\n\n');

// A repository whose main has the renames and the split README, and a branch from before them that
// edits what main renamed, adds code with the old names and edits the Features bullets.
beforeEach(() => {
	repo = mkdtempSync(join(tmpdir(), 'merge-main-'));
	git('init', '-q', '-b', 'main');
	git('config', 'user.email', 'test@example.com');
	git('config', 'user.name', 'Test');
	git('config', 'commit.gpgsign', 'false');
	write('crates/a/Cargo.toml', '[package]\nname = "a"\n');
	write('crates/a/src/lib.rs', '#[hegel::test]\nfn keeps_order() {\n    check(1);\n}\n');
	write('crates/a/src/old.rs', '#[hegel::test]\nfn old_case() {}\n');
	write('apps/web/src/lib/bookings.ts', 'export const x = 1;\n');
	write('apps/web/src/routes/a.ts', "import { x } from '$lib/bookings';\n");
	write('notes.txt', 'notes\n');
	write(
		'apps/web/README.md',
		readme(
			bullets(
				'`/tools` lists the tools, see [bookings](src/lib/bookings.ts).',
				'- "Report a bug" opens a form.\n- Posting asks for one more OAuth scope.',
				'Flags are kept in flags/.'
			)
		)
	);
	commit('start');
	git('branch', 'branch');

	write('crates/a/src/lib.rs', '#[hegel::test]\nfn prop_keeps_order() {\n    check(1);\n}\n');
	write('crates/a/src/old.rs', '#[hegel::test]\nfn prop_old_case() {}\n');
	git('mv', 'apps/web/src/lib/bookings.ts', 'apps/web/src/lib/bookings.page-only.ts');
	write('apps/web/src/routes/a.ts', "import { x } from '$lib/bookings.page-only';\n");
	write('notes.txt', 'notes from main\n');
	write('apps/web/README.md', readme('Each feature has a file in [`docs/features/`](docs/features/).'));
	write(
		'apps/web/docs/features/tools/tools.md',
		'# Tools\n\n`/tools` lists the tools, see [bookings](../../../src/lib/bookings.page-only.ts).\n'
	);
	write(
		'apps/web/docs/features/bug-reports/report-a-bug.md',
		'# Report a bug\n\n"Report a bug" opens a form, from the footer.\n'
	);
	write(
		'apps/web/docs/features/bug-reports/posting-scope.md',
		'# The posting scope\n\nPosting asks for one more OAuth scope.\n'
	);
	commit('renames and the split');

	git('switch', '-q', 'branch');
	write('crates/a/src/lib.rs', '#[hegel::test]\nfn keeps_order() {\n    check(2);\n}\n');
	write('crates/a/src/more.rs', '#[hegel::test]\nfn grows_back() {}\n');
	rmSync(join(repo, 'crates/a/src/old.rs'));
	write('crates/a/README.md', 'Checks `grows_back`, and draft_case when it lands.\n');
	write('apps/web/src/routes/a.ts', "import { x, y } from '$lib/bookings';\n");
	write(
		'apps/web/src/routes/b.ts',
		"import { x } from '$lib/bookings';\nimport { d } from '$lib/form-draft.svelte';\n"
	);
	write('apps/web/src/lib/form-draft.svelte.ts', 'export const d = 1;\n');
	write('notes.txt', 'notes from the branch\n');
	write('docs/links.md', 'See [tools](../apps/web/README.md#tools) for grows_back.\n');
	write(
		'apps/web/README.md',
		readme(
			bullets(
				'`/tools` lists every tool, see [bookings](src/lib/bookings.ts).',
				'- "Report a bug" opens a dialog.',
				'Flags are kept in flags/, one file each.',
				'### Fans\n\n- Follow a project to hear of its shows.'
			),
			'Run it now.'
		)
	);
	commit('branch work');
});

afterEach(() => rmSync(repo, { recursive: true, force: true }));

// Merges main without committing, as a person would before running the tool. It stops on conflicts.
const merge = () => {
	try {
		git('merge', '--no-commit', '-q', 'main');
	} catch {
		// Conflicts, which the tool is for.
	}
};

describe('a merge of main into a branch from before its renames', () => {
	it("resolves what's only renamed, renames the branch's code, moves its feature edits and leaves the rest", () => {
		merge();
		// Not part of the merge, so its test's name isn't renamed anywhere.
		write('crates/a/src/draft.rs', '#[hegel::test]\nfn draft_case() {}\n');

		expect(run()).toEqual({
			ok: true,
			log: [
				"took this branch's side where main only renamed: apps/web/src/routes/a.ts, crates/a/src/lib.rs, crates/a/src/old.rs",
				"moved this branch's edits to the Features bullets into apps/web/docs/features/tools/tools.md, apps/web/docs/features/bug-reports/report-a-bug.md, apps/web/docs/features/bug-reports/posting-scope.md, apps/web/docs/features/flags/flags.md, apps/web/docs/features/fans/follows.md, and took main's Features section in apps/web/README.md",
				'moved the page-only modules: apps/web/src/lib/form-draft.svelte.ts -> apps/web/src/lib/form-draft.page-only.svelte.ts',
				'old test names left outside their crates, for a look:\ndocs/links.md:1: See [tools](../apps/web/docs/features/tools/) for grows_back.',
				'renamed in 6 files: apps/web/src/routes/a.ts, apps/web/src/routes/b.ts, crates/a/README.md, crates/a/src/lib.rs, crates/a/src/more.rs, docs/links.md',
				'still unmerged, for you to resolve: apps/web/docs/features/bug-reports/report-a-bug.md, apps/web/docs/features/flags/flags.md, notes.txt'
			]
		});

		expect(read('crates/a/src/lib.rs')).toBe('#[hegel::test]\nfn prop_keeps_order() {\n    check(2);\n}\n');
		expect(read('crates/a/src/more.rs')).toBe('#[hegel::test]\nfn prop_grows_back() {}\n');
		expect(existsSync(join(repo, 'crates/a/src/old.rs'))).toBe(false);
		expect(read('crates/a/README.md')).toBe('Checks `prop_grows_back`, and draft_case when it lands.\n');
		expect(read('apps/web/src/routes/a.ts')).toBe("import { x, y } from '$lib/bookings.page-only';\n");
		expect(read('apps/web/src/routes/b.ts')).toBe(
			"import { x } from '$lib/bookings.page-only';\nimport { d } from '$lib/form-draft.page-only.svelte';\n"
		);
		expect(existsSync(join(repo, 'apps/web/src/lib/form-draft.svelte.ts'))).toBe(false);
		expect(read('apps/web/src/lib/form-draft.page-only.svelte.ts')).toBe('export const d = 1;\n');

		expect(read('apps/web/README.md')).toBe(
			readme('Each feature has a file in [`docs/features/`](docs/features/).', 'Run it now.')
		);
		expect(read('apps/web/docs/features/tools/tools.md')).toBe(
			'# Tools\n\n`/tools` lists every tool, see [bookings](../../../src/lib/bookings.page-only.ts).\n'
		);
		expect(read('apps/web/docs/features/fans/follows.md')).toBe(
			'# Follows\n\nFollow a project to hear of its shows.\n'
		);
		expect(existsSync(join(repo, 'apps/web/docs/features/bug-reports/posting-scope.md'))).toBe(false);
		expect(read('apps/web/docs/features/bug-reports/report-a-bug.md')).toContain(
			'<<<<<<< HEAD\n"Report a bug" opens a dialog.\n=======\n"Report a bug" opens a form, from the footer.\n>>>>>>> MERGE_HEAD\n'
		);

		// Everything it changed is staged, except what still conflicts.
		expect(unmerged()).toEqual([
			'apps/web/docs/features/bug-reports/report-a-bug.md',
			'apps/web/docs/features/flags/flags.md',
			'notes.txt'
		]);
		expect(
			git('diff', '--name-only')
				.split('\n')
				.filter((path) => path && !unmerged().includes(path))
		).toEqual([]);
	});

	it('changes nothing when run again', () => {
		merge();
		run();

		const before = state();

		expect(run().log).toEqual([
			'still unmerged, for you to resolve: apps/web/docs/features/bug-reports/report-a-bug.md, apps/web/docs/features/flags/flags.md, notes.txt'
		]);
		expect(state()).toBe(before);
	});

	it('keeps the README unmerged when it conflicts outside the Features section too, and a second run changes nothing', () => {
		git('switch', '-q', 'main');
		write('apps/web/README.md', read('apps/web/README.md').replace('Run it.', 'Run it later.'));
		commit('setup');
		git('switch', '-q', 'branch');
		merge();
		run();

		const before = state();

		expect(unmerged()).toContain('apps/web/README.md');
		expect(read('apps/web/README.md')).toContain(
			'Each feature has a file in [`docs/features/`](docs/features/).\n\n## Setup\n\n<<<<<<< HEAD\nRun it now.\n=======\nRun it later.\n>>>>>>> MERGE_HEAD\n'
		);
		expect(run().log).toEqual([
			'still unmerged, for you to resolve: apps/web/README.md, apps/web/docs/features/bug-reports/report-a-bug.md, apps/web/docs/features/flags/flags.md, notes.txt'
		]);
		expect(state()).toBe(before);
	});

	it("leaves the README to a person when the branch has a bullet it can't place", () => {
		write(
			'apps/web/README.md',
			read('apps/web/README.md').replace('## Setup', '### Venues\n\n- A venue page.\n\n## Setup')
		);
		commit('venues');
		merge();

		const { ok, log } = run();

		expect(ok).toBe(false);
		expect(log).toContain(
			"apps/web/README.md: these aren't in tools/merge-main/sections.ts, so the README is left to you:\n### Venues"
		);
		expect(unmerged()).toContain('apps/web/README.md');
		expect(existsSync(join(repo, 'apps/web/docs/features/fans/follows.md'))).toBe(false);
	});

	it('refuses to rename a test whose name another item in its file has, saying where', () => {
		write('crates/a/src/more.rs', '#[hegel::test]\nfn grows_back() {}\nconst grows_back: u8 = 1;\n');
		write('crates/a/src/helpers.rs', 'fn grows_back() {}\n');
		commit('a collision');
		merge();

		const { ok, log } = run();

		expect(ok).toBe(false);
		expect(log).toContain(
			"these have a hegel test's name in the test's own file, so rename them by hand first:\ncrates/a/src/more.rs:3: const grows_back: u8 = 1;"
		);
		expect(log).toContain(
			'grows_back: also crates/a/src/helpers.rs:1: fn grows_back() {}, so renamed only in crates/a/src/more.rs'
		);
		expect(read('crates/a/src/more.rs')).toContain('fn grows_back()');
	});
});

it('does nothing to a merge with nothing of the renames in it', () => {
	git('switch', '-q', 'main');
	git('switch', '-q', '-c', 'later');
	write('other.txt', 'more\n');
	commit('later');
	git('switch', '-q', 'main');
	git('merge', '--no-commit', '--no-ff', '-q', 'later');

	const before = state();

	expect(run()).toEqual({ ok: true, log: ['nothing to do'] });
	expect(state()).toBe(before);
});
