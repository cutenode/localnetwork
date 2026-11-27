import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { type Target, corpora, headerOf, problems, readTargets, run, select } from './targets.ts';

const repo = resolve(import.meta.dirname, '../..');

describe("the repository's fuzz targets", () => {
	const targets = readTargets(repo);

	// Tracked files and new ones not yet added, as tools/policy/inputs.ts lists them.
	const files = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], {
		cwd: repo,
		encoding: 'utf8'
	})
		.split('\n')
		.filter(Boolean);

	it('each name what they fuzz, in globs that match files', () => {
		expect(targets.size).toBeGreaterThan(0);
		expect(problems(targets, files)).toEqual([]);
	});

	it('run only the targets whose paths a change to loclib-anlz reaches', () => {
		expect(select(targets, ['crates/loclib-anlz/src/lib.rs'])).toEqual(['anlz']);
	});

	it('run none for a change to docs alone', () => {
		expect(select(targets, ['crates/loclib-anlz/docs/formats.md', 'fuzz/notes.md'])).toEqual([]);
	});
});

describe('headerOf', () => {
	it('reads the leading paths lines, which are measured too', () => {
		expect(headerOf('// paths: a/**\n// paths: b.rs\n//! Docs.\n// paths: c.rs\n')).toEqual({
			paths: ['a/**', 'b.rs'],
			coverage: ['a/**', 'b.rs']
		});
	});

	it('measures the coverage lines alone when there are any, among the paths or after them', () => {
		expect(headerOf('// paths: a.rs\n// coverage: b.rs\n// paths: b.rs\n// coverage: c/**\n//! Docs.\n')).toEqual({
			paths: ['a.rs', 'b.rs'],
			coverage: ['b.rs', 'c/**']
		});
	});

	it.each(['// paths: b.rs ', '// coverage: b.rs c.rs', '// paths:', '// coverage:b.rs'])('refuses %j', (line) => {
		expect(() => headerOf(`// paths: a.rs\n${line}\n`)).toThrow(`not one glob: ${line}`);
	});

	it('finds none in a target that starts with anything else', () => {
		expect(headerOf('//! Docs.\n// paths: a/**\n// coverage: a/b.rs\n')).toEqual({ paths: [], coverage: [] });
	});
});

/** Targets with the paths given, all of them measured. */
const targeting = (paths: Record<string, string[]>) =>
	new Map(Object.entries(paths).map(([name, globs]): [string, Target] => [name, { paths: globs, coverage: globs }]));

describe('select', () => {
	const targets = targeting({
		anlz: ['fuzz/fuzz_targets/anlz.rs', 'crates/loclib-anlz/src/**', 'crates/locallibrary/tests/fixtures/anlz/**'],
		wav: ['fuzz/fuzz_targets/wav.rs', 'crates/locallibrary/src/recordings.rs']
	});

	it('picks a target for its paths, whatever it measures', () => {
		const wav = { paths: ['crates/locallibrary/src/{recordings,recordings/wav}.rs'], coverage: ['crates/gone.rs'] };

		expect(select(new Map([['wav', wav]]), ['crates/locallibrary/src/recordings.rs'])).toEqual(['wav']);
		expect(select(new Map([['wav', wav]]), ['crates/gone.rs'])).toEqual([]);
	});

	it.each([
		[['crates/loclib-anlz/src/grid.rs'], ['anlz']],
		[
			['crates/locallibrary/src/recordings.rs', 'crates/loclib-anlz/src/lib.rs'],
			['anlz', 'wav']
		],
		[['fuzz/fuzz_targets/wav.rs'], ['wav']],
		// A seed beside the tests still counts, where the tests' code doesn't.
		[['crates/locallibrary/tests/fixtures/anlz/ANLZ0000.DAT'], ['anlz']],
		[['crates/loclib-anlz/src/tests.rs', 'crates/loclib-anlz/src/grid/tests/rounding.rs'], []],
		[['crates/loclib-anlz/src/README.md'], []],
		[['crates/locallibrary/src/device.rs'], []],
		[[], []]
	])('picks for %j: %j', (changed, chosen) => {
		expect(select(targets, changed)).toEqual(chosen);
	});

	it.each([
		'fuzz/Cargo.lock',
		'.cargo/config.toml',
		'.github/actions/fuzz/action.yml',
		'.github/workflows/locallibrary-ci.yml',
		'tools/fuzz/targets.ts'
	])('picks every target for %s', (file) => {
		expect(select(targets, [file])).toEqual(['anlz', 'wav']);
	});
});

describe('problems', () => {
	const files = ['crates/loclib-anlz/src/lib.rs', 'crates/loclib-anlz/src/tests.rs', 'fuzz/fuzz_targets/anlz.rs'];

	it('passes globs that each match a file', () => {
		expect(problems(targeting({ anlz: ['fuzz/fuzz_targets/anlz.rs', 'crates/loclib-anlz/src/**'] }), files)).toEqual(
			[]
		);
	});

	it('names a target without paths', () => {
		expect(problems(targeting({ anlz: [] }), files)).toEqual([
			'fuzz/fuzz_targets/anlz.rs: start it with `// paths: <glob>` lines for what it fuzzes'
		]);
	});

	it('names a glob that matches nothing, or only what every target leaves out', () => {
		expect(
			problems(
				targeting({
					anlz: ['crates/loclib-anlz/src/tests.rs', 'crates/loclib-anlz/src/lib.rs', 'crates/gone/**']
				}),
				files
			)
		).toEqual([
			'fuzz/fuzz_targets/anlz.rs: `crates/loclib-anlz/src/tests.rs` matches no file',
			'fuzz/fuzz_targets/anlz.rs: `crates/gone/**` matches no file'
		]);
	});

	it('names a measured glob that matches nothing', () => {
		const anlz = { paths: ['crates/loclib-anlz/src/**'], coverage: ['crates/loclib-anlz/src/grid.rs'] };

		expect(problems(new Map([['anlz', anlz]]), files)).toEqual([
			'fuzz/fuzz_targets/anlz.rs: `crates/loclib-anlz/src/grid.rs` matches no file'
		]);
	});

	it('names a measured file that a change to runs nothing for', () => {
		const anlz = { paths: ['fuzz/fuzz_targets/anlz.rs'], coverage: ['crates/loclib-anlz/src/**'] };

		expect(problems(new Map([['anlz', anlz]]), files)).toEqual([
			'fuzz/fuzz_targets/anlz.rs: `crates/loclib-anlz/src/lib.rs` is measured, so its `// paths:` have to name it too'
		]);
	});
});

describe('corpora', () => {
	const config = [
		'fuzz-pdb = "fuzz run pdb fuzz/corpus/pdb crates/a/rekordbox crates/b/rekordbox -- -create_missing_dirs=1 -dict=fuzz/dicts/pdb.dict"',
		'fuzz-pdb_edit = "fuzz run pdb_edit fuzz/corpus/pdb_edit -- -create_missing_dirs=1"'
	].join('\n');

	it('gives the folders before the libFuzzer options', () => {
		expect(corpora(config, 'pdb')).toEqual(['fuzz/corpus/pdb', 'crates/a/rekordbox', 'crates/b/rekordbox']);
		expect(corpora(config, 'pdb_edit')).toEqual(['fuzz/corpus/pdb_edit']);
	});

	it('fails for a target without an alias', () => {
		expect(() => corpora(config, 'pd')).toThrow('.cargo/config.toml has no fuzz-pd alias');
	});
});

describe('run', () => {
	let root: string;

	const git = (...args: string[]) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();

	const write = (path: string, text: string) => {
		mkdirSync(join(root, path, '..'), { recursive: true });
		writeFileSync(join(root, path), text);
	};

	beforeEach(() => {
		root = mkdtempSync(join(tmpdir(), 'fuzz-targets-'));
		write('fuzz/fuzz_targets/anlz.rs', '// paths: crates/loclib-anlz/src/**\n');
		write('fuzz/fuzz_targets/wav.rs', '// paths: crates/locallibrary/src/recordings.rs\n');
		write('fuzz/fuzz_targets/notes.txt', 'not a target');
		write('.cargo/config.toml', 'fuzz-wav = "fuzz run wav fuzz/corpus/wav fuzz/seeds/wav -- -create_missing_dirs=1"\n');
		write('crates/loclib-anlz/src/lib.rs', '');
		git('init', '-q');
		git('add', '.');
		git('-c', 'user.name=t', '-c', 'user.email=t@example.com', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'base');
	});

	afterEach(() => rmSync(root, { recursive: true, force: true }));

	it('selects every target without a base', () => {
		expect(run(['select'], root)).toEqual({ lines: ['anlz wav'], ok: true });
	});

	it('selects the targets the commits since the base reach', () => {
		const base = git('rev-parse', 'HEAD');

		write('crates/loclib-anlz/src/lib.rs', 'pub fn read() {}\n');
		git(
			'-c',
			'user.name=t',
			'-c',
			'user.email=t@example.com',
			'-c',
			'commit.gpgsign=false',
			'commit',
			'-qam',
			'change'
		);

		expect(run(['select', base], root)).toEqual({ lines: ['anlz'], ok: true });
		expect(run(['select', 'HEAD'], root)).toEqual({ lines: [''], ok: true });
	});

	it("prints a target's corpora a line each", () => {
		expect(run(['corpora', 'wav'], root)).toEqual({ lines: ['fuzz/corpus/wav', 'fuzz/seeds/wav'], ok: true });
	});

	it.each([[[]], [['corpora']], [['select', 'HEAD', 'extra']], [['corpora', 'wav', 'extra']], [['list']]])(
		'gives the usage for %j',
		(args) => {
			expect(run(args, root)).toEqual({ lines: ['usage: cli.ts select [<base>] | corpora <target>'], ok: false });
		}
	);
});
