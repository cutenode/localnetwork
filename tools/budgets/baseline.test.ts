import { execFileSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { filterPaths, run, selects } from './baseline.ts';

describe('selects', () => {
	const paths = ['crates/**', 'Cargo.lock', '!**/*.md', '!**/tests/**'];

	it('takes a file a glob matches and no exclusion does', () => {
		expect(selects(paths, ['crates/a/src/lib.rs'])).toBe(true);
		expect(selects(paths, ['README.md', 'Cargo.lock'])).toBe(true);
	});
	it('leaves out excluded and unmatched files', () => {
		expect(selects(paths, ['crates/a/README.md', 'crates/a/tests/it.rs', 'apps/web/x.ts', ''])).toBe(false);
	});
});

describe('filterPaths', () => {
	it("reads a filter's paths from ci.yml", () => {
		expect(filterPaths('locallibrary_release')).toContain('Cargo.lock');
		expect(filterPaths('locallibrary')).toContain('crates/**');
	});
	it('throws for a filter ci.yml lacks', () => {
		expect(() => filterPaths('no_such_filter')).toThrow('ci.yml has no no_such_filter paths filter');
	});
});

describe('the command', () => {
	const start = process.cwd();
	const path = process.env.PATH;
	let dir: string;

	const git = (...args: string[]) =>
		execFileSync('git', ['-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
			encoding: 'utf8'
		}).trim();

	const commit = (message: string) => {
		git('add', '.');
		git('commit', '-qm', message);

		return git('rev-parse', 'HEAD');
	};

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'baseline-'));
		process.chdir(dir);
	});
	afterEach(() => {
		process.chdir(start);
		process.env.PATH = path;
		rmSync(dir, { recursive: true });
	});

	describe('baseline', () => {
		// A gh that lists the runs named in runs/<sha> and downloads an artifact from runs whose
		// artifacts/<run> file exists, writing that file's contents as size.json.
		const fakeGh = (runs: Record<string, string[]>, artifacts: string[]) => {
			mkdirSync('bin');
			mkdirSync('runs');
			mkdirSync('artifacts');

			for (const [sha, ids] of Object.entries(runs)) {
				writeFileSync(join('runs', sha), ids.map((id) => `${id}\n`).join(''));
			}

			for (const id of artifacts) writeFileSync(join('artifacts', id), `{"binary": ${id}}`);

			writeFileSync(
				'bin/gh',
				`#!/bin/sh
root=${JSON.stringify(dir)}
case "$1 $2" in
  "run list") cat "$root/runs/$6" 2>/dev/null || true ;;
  "run download") [ -f "$root/artifacts/$3" ] || exit 1; mkdir -p "$7"; cp "$root/artifacts/$3" "$7/size.json" ;;
esac
`
			);
			chmodSync('bin/gh', 0o755);
			process.env.PATH = `${join(dir, 'bin')}:${path}`;
		};

		const repo = () => {
			mkdirSync('.github/workflows', { recursive: true });
			writeFileSync(
				'.github/workflows/ci.yml',
				`jobs:
  changes:
    steps:
      - uses: actions/checkout
      - id: filter
        with:
          filters: |
            sizes:
              - 'src/**'
              - '!**/*.md'
`
			);
			mkdirSync('src');
			git('init', '-q');
		};

		it("downloads the artifact of the commit's own run, past a run without it", () => {
			repo();
			const sha = commit('one');
			fakeGh({ [sha]: ['1', '2'] }, ['2']);
			expect(run(['baseline', sha, 'sizes', 'budgets', 'out'])).toEqual({
				lines: ['found=true', `from=${sha}`],
				ok: true
			});
			expect(JSON.parse(readFileSync('out/size.json', 'utf8'))).toEqual({ binary: 2 });
		});
		it("walks back past commits that don't change the filter's paths", () => {
			repo();
			writeFileSync('src/a.rs', '1');
			const measured = commit('measured');
			writeFileSync('src/a.md', 'docs');
			const docs = commit('docs only');
			fakeGh({ [measured]: ['7'] }, ['7']);
			expect(run(['baseline', docs, 'sizes', 'budgets', 'out']).lines).toEqual(['found=true', `from=${measured}`]);
		});
		it('stops at a commit that changed what the budgets measure', () => {
			repo();
			writeFileSync('src/a.rs', '1');
			const measured = commit('measured');
			writeFileSync('src/a.rs', '2');
			const changed = commit('code');
			fakeGh({ [measured]: ['7'] }, ['7']);
			expect(run(['baseline', changed, 'sizes', 'budgets', 'out']).lines).toEqual(['found=false']);
		});
		it('finds nothing when no run kept the artifact', () => {
			repo();
			const sha = commit('one');
			fakeGh({}, []);
			expect(run(['baseline', sha, 'sizes', 'budgets', 'out']).lines).toEqual(['found=false']);
		});
	});

	it.each([[[]], [['baseline', 'sha', 'filter', 'artifact']], [['check', 'a', 'b', 'c']]])(
		'%j prints how to use it',
		(args) => {
			expect(run(args).ok).toBe(false);
		}
	);
});
