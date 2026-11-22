import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { EVERY_CHANGE, changedFiles, picks, verifyChanged } from './changed.ts';
import { steps } from './run.ts';

const dir = mkdtempSync(join(tmpdir(), 'verify-changed-'));

afterAll(() => rmSync(dir, { recursive: true, force: true }));

const commands = (files: string[]) => picks(files).map(({ command }) => command);

// The root verify's own steps, as steps.json lists them.
const root = (steps.get('verify') ?? []).filter((step) => !step.startsWith('pnpm -r '));

describe('picks', () => {
	it("picks the website's checks for a change to it, the root's own, and the repo checks CI runs on every change", () => {
		expect(picks(['apps/web/src/lib/x.ts'])).toEqual([
			{ command: 'pnpm -C apps/web verify', file: 'apps/web/src/lib/x.ts' },
			...root.map((command) => ({ command, file: 'apps/web/src/lib/x.ts' })),
			{ command: 'pnpm verify:repo', file: 'apps/web/src/lib/x.ts' }
		]);
	});

	it("runs the root verify's own steps, not its `pnpm -r`, for what the repo checks' filter takes", () => {
		expect(root).toContain('pnpm lint:tools');
		expect(commands(['tools/verify/run.ts'])).toEqual(expect.arrayContaining(root));
		expect(commands(['tools/verify/run.ts'])).not.toContain('pnpm -r --no-bail verify');
	});

	it('picks verify:rust for a change to a crate', () => {
		expect(commands(['crates/loclib-model/src/lib.rs'])).toEqual([...root, 'pnpm verify:rust', 'pnpm verify:repo']);
	});

	it("picks verify:rust for the Markdown and config its jobs read: a crate's README, the docs its tests read, dist's config", () => {
		expect(commands(['crates/loclib-model/README.md'])).toEqual(['pnpm verify:rust', 'pnpm verify:repo']);
		expect(commands(['apps/locallibrary/docs/plugins.md'])).toContain('pnpm verify:rust');
		expect(commands(['dist-workspace.toml'])).toEqual([...root, 'pnpm verify:rust', 'pnpm verify:repo']);
	});

	it('picks only the repo checks for Markdown no tool reads', () => {
		expect(commands(['docs/notes.md'])).toEqual(['pnpm verify:repo']);
	});

	it('names the first file that picked each script', () => {
		expect(picks(['docs/a.md', 'packages/atstore/src/x.ts', 'packages/atstore/src/y.ts'])).toEqual([
			{ command: 'pnpm -C packages/atstore verify', file: 'packages/atstore/src/x.ts' },
			...root.map((command) => ({ command, file: 'packages/atstore/src/x.ts' })),
			{ command: 'pnpm verify:repo', file: 'docs/a.md' }
		]);
	});
});

describe('changedFiles', () => {
	const git = (cwd: string, args: string[], input?: string) =>
		execFileSync('git', args, { cwd, input, encoding: 'utf8' });

	// origin/main with base.txt, and main a commit on from it with branch.txt. fast-import writes the
	// commits as the stream says, with no identity or signing of the machine's own.
	const repo = (name: string) => {
		const root = join(dir, name);

		git(dir, ['init', '-q', '-b', 'main', root]);

		return root;
	};

	const stream = (file: string, ref: string, from = '') =>
		`commit ${ref}\ncommitter t <t@t> 0 +0000\ndata 0\n${from}M 644 inline ${file}\ndata 2\n${file[0]}\n\n`;

	it('lists what the branch and the working tree change, new files too, once each', () => {
		const root = repo('changed');

		git(
			root,
			['fast-import', '--quiet'],
			stream('base.txt', 'refs/remotes/origin/main') +
				stream('branch.txt', 'refs/heads/main', 'from refs/remotes/origin/main\n')
		);
		git(root, ['reset', '-q', '--hard']);
		writeFileSync(join(root, 'base.txt'), 'changed');
		writeFileSync(join(root, 'branch.txt'), 'changed');
		writeFileSync(join(root, 'new.txt'), 'new');

		expect(changedFiles(root)).toEqual({ files: ['branch.txt', 'base.txt', 'new.txt'] });
	});

	it('says what to do without origin/main, in one line', () => {
		expect(verifyChanged(changedFiles(repo('bare')))).toEqual({
			lines: ["verify:changed compares with origin/main, which this checkout doesn't have: git fetch origin main"],
			ok: false
		});
	});
});

describe('verifyChanged', () => {
	const marker = join(dir, 'ran');

	it('runs only the scripts the changes picked, and says what picked them', () => {
		const scripts = {
			'exit 1': ['web'],
			[`echo > "${marker}"`]: ['localtoolbox'],
			'exit 0': [EVERY_CHANGE]
		};

		const { lines, ok } = verifyChanged({ files: ['apps/localtoolbox/src/x.ts'] }, scripts);

		expect(ok).toBe(true);
		expect(existsSync(marker)).toBe(true);
		expect(lines.slice(0, 2)).toEqual([
			`echo > "${marker}": picked by apps/localtoolbox/src/x.ts`,
			'exit 0: picked by apps/localtoolbox/src/x.ts'
		]);
		expect(lines).toContain('verify:changed: every step passed');
	});

	it('runs nothing when nothing changed', () => {
		expect(verifyChanged({ files: [] })).toEqual({
			lines: ['Nothing changed from origin/main: nothing to verify'],
			ok: true
		});
	});
});
