import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { path } from './fakes.ts';
import { WINDOWS_CLIPPY, preflight } from './tools.ts';

const repoTools = {
	docker: '',
	shellcheck: 'version: 0.0.1',
	typos: 'typos-cli 0.0.1',
	taplo: 'taplo 0.0.1',
	go: '',
	lychee: ''
};

describe('preflight', () => {
	it("stops on each tool the script needs that isn't there, with how to install it", () => {
		const { taplo: _, ...withoutTaplo } = repoTools;

		expect(preflight('verify:repo', path(withoutTaplo)).problems).toEqual([
			'verify:repo needs taplo: brew install taplo'
		]);
		// At the versions CI pins: locallibrary-ci.yml's for nextest and shear, dist-workspace.toml's for dist.
		expect(preflight('verify:rust', { PATH: '' }).problems).toEqual([
			expect.stringMatching(/^verify:rust needs cargo-nextest: cargo binstall cargo-nextest@\d+\.\d+\.\d+$/),
			expect.stringMatching(/^verify:rust needs cargo-shear: cargo binstall cargo-shear@\d+\.\d+\.\d+$/),
			expect.stringMatching(/^verify:rust needs dist: cargo binstall cargo-dist@\d+\.\d+\.\d+$/)
		]);
	});

	it("stops when a pin's file no longer says it where the check looks", () => {
		const root = mkdtempSync(join(tmpdir(), 'verify-pins-'));

		try {
			mkdirSync(join(root, '.github/workflows'), { recursive: true });
			writeFileSync(join(root, '.github/workflows/locallibrary-ci.yml'), 'tool: cargo-shear@1.0.0\n');
			writeFileSync(join(root, 'dist-workspace.toml'), 'cargo-dist-version = "1.0.0"\n');

			expect(preflight('verify:rust', path({ cargo: '', dist: '' }), root).problems).toEqual([
				'.github/workflows/locallibrary-ci.yml no longer pins cargo-nextest where tools/verify/tools.ts looks (/tool: cargo-nextest@([\\d.]+)/)'
			]);

			// The install lines name the versions the files pin.
			writeFileSync(
				join(root, '.github/workflows/locallibrary-ci.yml'),
				'tool: cargo-shear@1.0.0\ntool: cargo-nextest@2.0.0\n'
			);

			expect(preflight('verify:rust', { PATH: '' }, root).problems).toEqual([
				'verify:rust needs cargo-nextest: cargo binstall cargo-nextest@2.0.0',
				'verify:rust needs cargo-shear: cargo binstall cargo-shear@1.0.0',
				'verify:rust needs dist: cargo binstall cargo-dist@1.0.0'
			]);
		} finally {
			rmSync(root, { recursive: true, force: true });
		}
	});

	it("warns when a pinned tool isn't the version CI pins, read from the workflow", () => {
		const { problems, notes } = preflight('verify:repo', path({ ...repoTools, shellcheck: 'no version' }));

		expect(problems).toEqual([]);
		expect(notes).toEqual([
			expect.stringMatching(/^ShellCheck unknown isn't CI's \d+\.\d+\.\d+ \(\.github\/workflows\/repo-checks\.yml\)/),
			expect.stringMatching(/^typos 0\.0\.1 isn't CI's \d+\.\d+\.\d+ \(\.github\/workflows\/repo-checks\.yml\)/),
			expect.stringMatching(/^taplo 0\.0\.1 isn't CI's \d+\.\d+\.\d+ \(\.github\/workflows\/repo-checks\.yml\)/)
		]);

		// The same typos at the version CI pins, which the warning named.
		const pinned = / isn't CI's (\S+) /.exec(notes[1] ?? '')?.[1];
		const atPin = preflight('verify:repo', path({ ...repoTools, typos: `typos-cli ${pinned}` }));

		expect(atPin.notes).not.toContainEqual(expect.stringMatching(/^typos /));
		expect(atPin.notes).toHaveLength(2);
	});

	it('says what each skip leaves unchecked, and leaves out the step that would skip', () => {
		const rust = { cargo: '', dist: '', rustup: 'aarch64-apple-darwin' };

		expect(preflight('verify:rust', path(rust))).toEqual({
			problems: [],
			notes: [expect.stringContaining('DJs syncing drives on Windows'), expect.stringContaining('SQLCipher')],
			skipped: [WINDOWS_CLIPPY]
		});
		expect(preflight('verify:rust', path({ ...rust, rustup: 'x86_64-pc-windows-gnu' })).skipped).toEqual([
			WINDOWS_CLIPPY
		]);
		expect(
			preflight(
				'verify:rust',
				path({ ...rust, rustup: 'x86_64-pc-windows-gnu', 'x86_64-w64-mingw32-gcc': '', sqlcipher: '' })
			)
		).toEqual({ problems: [], notes: [], skipped: [] });
	});

	it('needs nothing for a script with no tools of its own', () => {
		expect(preflight('verify', { PATH: '' })).toEqual({ problems: [], notes: [], skipped: [] });
	});

	it("skips a step that's in steps.json", () => {
		const steps: unknown = JSON.parse(readFileSync(join(import.meta.dirname, 'steps.json'), 'utf8'));

		expect(steps).toMatchObject({ 'verify:rust': expect.arrayContaining([WINDOWS_CLIPPY]) });
	});
});
