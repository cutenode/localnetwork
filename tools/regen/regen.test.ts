import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { committed, driver, problems, regenerate, unpushable } from './regen.ts';

const CLI = resolve(import.meta.dirname, 'cli.ts');

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

// The pre-push hook's stdin for a push of `ref`.
const pushing = (ref: string) => `refs/heads/x ${git('rev-parse', ref).trim()} refs/heads/x ${'0'.repeat(40)}\n`;

const staged = () => git('diff', '--cached', '--name-only').split('\n').filter(Boolean);

// Changes a.txt on both branches too, so merging stops on its conflict.
const conflictOnA = () => {
	git('switch', '-q', 'theirs');
	write('a.txt', 'a3\n');
	commit('a3');
	git('switch', '-q', 'main');
};

// A repository whose docs/rpc/schema.json is made from a.txt and b.txt by tools/regen/stub.sh, and
// two branches that each change one of them and regenerate it. The stub also writes a new file of
// its own, one another command owns and one nobody does, which it mustn't stage.
beforeEach(() => {
	repo = mkdtempSync(join(tmpdir(), 'regen-'));
	git('init', '-q', '-b', 'main');
	git('config', 'user.email', 'test@example.com');
	git('config', 'user.name', 'Test');
	git('config', 'commit.gpgsign', 'false');
	git('config', 'merge.regen.driver', `node ${CLI} driver %P`);
	write(
		'tools/regen/stub.sh',
		'cat a.txt b.txt > docs/rpc/schema.json\necho made > docs/rpc/new.json\necho other > notes.md\necho other > docs/rpc/other.txt\n'
	);
	write('docs/rpc/.gitattributes', '*.json merge=regen regen=stub\nother.txt merge=regen regen=other\n');
	write('a.txt', 'a\n');
	write('b.txt', 'b\n');
	write('docs/rpc/schema.json', 'a\nb\n');
	commit('start');
	git('switch', '-q', '-c', 'theirs');
	write('b.txt', 'b2\n');
	write('docs/rpc/schema.json', 'a\nb2\n');
	commit('b2');
	git('switch', '-q', 'main');
	write('a.txt', 'a2\n');
	write('docs/rpc/schema.json', 'a2\nb\n');
	commit('a2');
});

afterEach(() => rmSync(repo, { recursive: true, force: true }));

const log: string[] = [];

beforeEach(() => {
	log.length = 0;
});

describe('a merge of two changes to a generated file', () => {
	it("goes through with this branch's copy, then regenerates the file and stages it", () => {
		git('merge', '-q', '--no-edit', 'theirs');
		expect(read('docs/rpc/schema.json')).toBe('a2\nb\n');

		expect(regenerate(repo, (line) => log.push(line))).toBe(true);
		expect(read('docs/rpc/schema.json')).toBe('a2\nb2\n');
		expect(staged()).toEqual(['docs/rpc/new.json', 'docs/rpc/schema.json']);
		expect(log).toEqual([
			'Regenerated with tools/regen/stub.sh and staged, to commit: docs/rpc/new.json, docs/rpc/schema.json'
		]);
	});

	it('holds the push until the file is regenerated, then until that is committed', () => {
		git('merge', '-q', '--no-edit', 'theirs');

		expect(unpushable(repo, '')).toEqual({ pending: ['docs/rpc/schema.json'], uncommitted: [] });
		regenerate(repo, () => {});
		expect(unpushable(repo, pushing('HEAD'))).toEqual({
			pending: [],
			uncommitted: ['docs/rpc/new.json', 'docs/rpc/schema.json']
		});
		commit('regenerate');
		expect(unpushable(repo, pushing('HEAD'))).toEqual({ pending: [], uncommitted: [] });
		expect(existsSync(join(repo, '.git/regen/staged'))).toBe(false);
	});

	it('judges the commits pushed, not the worktree', () => {
		git('merge', '-q', '--no-edit', 'theirs');
		regenerate(repo, () => {});

		// A branch without the merge goes, and so does the merge's once it's committed, whatever is
		// changed after.
		expect(unpushable(repo, pushing('theirs')).uncommitted).toEqual([]);
		commit('regenerate');
		write('docs/rpc/schema.json', 'blessed again\n');
		expect(unpushable(repo, pushing('HEAD')).uncommitted).toEqual([]);
	});

	it('regenerates on the commit that concludes a merge stopped by a conflict', () => {
		conflictOnA();

		expect(spawnSync('git', ['merge', '-q', '--no-edit', 'theirs'], { cwd: repo }).status).toBe(1);
		write('a.txt', 'a4\n');
		commit('merge');
		expect(committed(repo, (line) => log.push(line))).toBe(true);
		expect(read('docs/rpc/schema.json')).toBe('a4\nb2\n');
		expect(staged()).toEqual(['docs/rpc/new.json', 'docs/rpc/schema.json']);
		expect(unpushable(repo, '').pending).toEqual([]);
	});

	it('leaves the note for pre-push in the middle of a rebase', () => {
		conflictOnA();
		git('switch', '-q', 'theirs');

		expect(spawnSync('git', ['rebase', 'main'], { cwd: repo }).status).toBe(1);
		expect(committed(repo, (line) => log.push(line))).toBe(true);
		expect(log).toEqual([]);
		expect(unpushable(repo, '').pending).toEqual(['docs/rpc/schema.json']);
		git('rebase', '--abort');
	});

	it('stages a rename among the files it writes, with its source', () => {
		write('tools/regen/stub.sh', 'git mv docs/rpc/schema.json docs/rpc/moved.json\n');
		commit('move it');
		git('merge', '-q', '--no-edit', 'theirs');

		expect(regenerate(repo, (line) => log.push(line))).toBe(true);
		expect(log).toEqual([
			'Regenerated with tools/regen/stub.sh and staged, to commit: docs/rpc/moved.json, docs/rpc/schema.json'
		]);
	});

	it("keeps its notes in a linked worktree's own git folder", () => {
		const worktree = `${repo}-worktree`;

		git('worktree', 'add', '-q', '--detach', worktree, 'main');

		try {
			execFileSync('git', ['merge', '-q', '--no-edit', 'theirs'], { cwd: worktree });

			const pending = execFileSync('git', ['rev-parse', '--git-path', 'regen/pending'], {
				cwd: worktree,
				encoding: 'utf8'
			}).trim();

			expect(readFileSync(pending, 'utf8')).toBe('docs/rpc/schema.json\n');
			expect(regenerate(worktree, (line) => log.push(line))).toBe(true);
			expect(execFileSync('git', ['diff', '--cached', '--name-only'], { cwd: worktree, encoding: 'utf8' })).toBe(
				'docs/rpc/new.json\ndocs/rpc/schema.json\n'
			);
		} finally {
			rmSync(worktree, { recursive: true, force: true });
		}
	});

	it('keeps the copy and names the command when it fails', () => {
		write('tools/regen/stub.sh', 'exit 1\n');
		commit('break the command');
		git('merge', '-q', '--no-edit', 'theirs');

		expect(regenerate(repo, (line) => log.push(line))).toBe(false);
		expect(read('docs/rpc/schema.json')).toBe('a2\nb\n');
		expect(log).toEqual([
			"tools/regen/stub.sh failed, so docs/rpc/schema.json keep this branch's copy: run it again, or CI's check fails"
		]);
		expect(unpushable(repo, pushing('HEAD'))).toEqual({ pending: [], uncommitted: [] });
	});

	it('conflicts as before without the driver', () => {
		git('config', '--unset', 'merge.regen.driver');

		expect(spawnSync('git', ['merge', '-q', '--no-edit', 'theirs'], { cwd: repo }).status).toBe(1);
	});

	it('stages nothing a command leaves as it was', () => {
		driver(repo, 'docs/rpc/schema.json');
		write('.gitattributes', 'b.txt merge=regen\n');
		driver(repo, 'b.txt');
		write('tools/regen/stub.sh', 'true\n');

		expect(regenerate(repo, (line) => log.push(line))).toBe(true);
		expect(log).toEqual([]);
		expect(regenerate(repo, (line) => log.push(line))).toBe(true);
	});
});

describe('problems', () => {
	it('passes a command and the files that name it', () => {
		expect(problems(repo)).toEqual([]);
	});

	it('finds a file without a command, a regen without merge=regen and a command nothing names', () => {
		write('docs/rpc/.gitattributes', '*.json merge=regen regen=gone\n');
		write('.gitattributes', 'a.txt regen=stub\nb.txt merge=regen\nc.txt merge=union\n');
		write('c.txt', 'c\n');
		write('tools/regen/spare.sh', 'true\n');
		commit('break it');

		expect(problems(repo)).toEqual([
			'a.txt: regen=stub needs merge=regen too',
			'b.txt: merge=regen needs regen=<name> and tools/regen/<name>.sh',
			'docs/rpc/schema.json: merge=regen needs regen=<name> and tools/regen/<name>.sh',
			'tools/regen/spare.sh: no file has regen=spare'
		]);
	});

	it("passes this repository's own", () => {
		expect(problems(resolve(import.meta.dirname, '../..'))).toEqual([]);
	});
});
