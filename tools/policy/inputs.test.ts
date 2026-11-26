import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { type CargoMetadata, configFiles, crateRoots, shellLines, writeInputs } from './inputs.ts';

describe('configFiles', () => {
	it('lists the config files the policies read, and nothing else', () => {
		const files = configFiles();
		expect(files).toEqual(
			expect.arrayContaining([
				'package.json',
				'.cargo/config.toml',
				'apps/web/Dockerfile',
				'crates/locallibrary/Cargo.toml',
				'.github/workflows/ci.yml',
				'tools/verify/steps.json',
				'.github/rulesets/protect-main.json',
				'lexicons/computer/localnetwork/dj/set.json'
			])
		);
		expect(files).not.toContain('README.md');
		expect(files).not.toContain('tools/policy/lib.rego');
	});
});

describe('writeInputs', () => {
	const scratch = mkdtempSync(join(tmpdir(), 'policy-test-'));
	afterAll(() => rmSync(scratch, { recursive: true, force: true }));
	const read = (name: string): string => readFileSync(join(scratch, name), 'utf8');

	// One run for every case: it runs harden twice and git once.
	let names: string[] = [];
	beforeAll(async () => {
		names = await writeInputs(scratch);
	});

	it('names every input it writes', () => {
		expect(names.sort()).toEqual([
			'Cargo.lock.toml',
			'contributing.json',
			'crate-roots.json',
			'harden.json',
			'labeler.json',
			'railway-project.json',
			'reads.json',
			'tracked-files.json'
		]);
	});

	it('lists every file the repository has', () => {
		expect(JSON.parse(read('tracked-files.json')).files).toEqual(
			expect.arrayContaining(['README.md', 'tools/policy/inputs.ts'])
		);
	});

	it('lists what projects read from outside their folders', () => {
		expect(JSON.parse(read('reads.json')).reads).toContainEqual(
			expect.objectContaining({ from: expect.any(String), path: expect.any(String) })
		);
	});

	it("lists each library's root and its crate attributes, by manifest", () => {
		const { roots } = JSON.parse(read('crate-roots.json'));
		expect(roots['crates/loclib-progress/Cargo.toml']).toEqual({
			root: 'crates/loclib-progress/src/lib.rs',
			attributes: expect.arrayContaining(['#![doc = include_str!("../README.md")]', '#![warn(missing_docs)]'])
		});
		// The CLI's package builds only a binary.
		expect(Object.keys(roots)).not.toContain('apps/locallibrary/Cargo.toml');
	});

	it('copies Cargo.lock as it is', () => {
		expect(read('Cargo.lock.toml')).toBe(readFileSync('Cargo.lock', 'utf8'));
	});

	it('evaluates the Railway project', () => {
		expect(JSON.parse(read('railway-project.json')).resources).toContainEqual(
			expect.objectContaining({ address: 'service.localnetwork' })
		);
	});

	it("lists CONTRIBUTING's commands", () => {
		expect(JSON.parse(read('contributing.json')).commands).toContainEqual(expect.stringMatching(/^pnpm verify:repo /));
	});

	it('builds the labeler config from .github/labeler/ and the crates', () => {
		const labeler = JSON.parse(read('labeler.json'));
		expect(Object.keys(labeler)).toEqual(expect.arrayContaining(['web', 'web: ui', 'locallibrary: transcode']));
		expect(labeler['locallibrary: transcode']).toEqual([
			{ 'changed-files': [{ 'any-glob-to-any-file': ['crates/loclib-transcode/**'] }] }
		]);
	});

	it("keeps harden's report for each package manager", () => {
		const { harden } = JSON.parse(read('harden.json'));
		expect(harden.map(({ manager, folder }: { manager: string; folder: string }) => [manager, folder])).toEqual([
			['pnpm', '.'],
			['npm', 'apps/locallibrary/bombadil']
		]);
		expect(harden[0].report.checks).toContainEqual(expect.objectContaining({ id: 'p_runner' }));
		expect(harden[1].report.checks).toContainEqual(expect.objectContaining({ id: 'n_runner' }));
	});
});

describe('crateRoots', () => {
	const workspace = mkdtempSync(join(tmpdir(), 'crate-roots-'));
	afterAll(() => rmSync(workspace, { recursive: true, force: true }));

	const library = (path: string, source: string): string => {
		const file = join(workspace, path);
		mkdirSync(dirname(file), { recursive: true });
		writeFileSync(file, source);

		return file;
	};

	it('reads the root where [lib] says, and only the attributes before its first item', () => {
		const own = library(
			'crates/own/lib/root.rs',
			[
				'//! A crate whose [lib] names its root.',
				'#![warn(missing_docs)]',
				'#![cfg_attr(docsrs, feature(doc_cfg))]',
				'',
				'mod inner {',
				'    #![allow(missing_docs)]',
				'}'
			].join('\n')
		);

		const macros = library('crates/macros/src/lib.rs', '#![allow(missing_docs)]\n');

		const metadata: CargoMetadata = {
			workspace_root: workspace,
			packages: [
				{ manifest_path: join(workspace, 'crates/own/Cargo.toml'), targets: [{ kind: ['lib'], src_path: own }] },
				{ manifest_path: join(workspace, 'apps/tool/Cargo.toml'), targets: [{ kind: ['bin'], src_path: own }] },
				{
					manifest_path: join(workspace, 'crates/macros/Cargo.toml'),
					targets: [{ kind: ['proc-macro'], src_path: macros }]
				}
			]
		};

		expect(crateRoots(metadata)).toEqual({
			'crates/own/Cargo.toml': {
				root: 'crates/own/lib/root.rs',
				attributes: ['#![warn(missing_docs)]', '#![cfg_attr(docsrs, feature(doc_cfg))]']
			},
			'crates/macros/Cargo.toml': { root: 'crates/macros/src/lib.rs', attributes: ['#![allow(missing_docs)]'] }
		});
	});
});

describe('shellLines', () => {
	it('takes each line of the bash and sh blocks, and nothing outside them', () => {
		const markdown = [
			'Run `pnpm outside`:',
			'```bash',
			'  pnpm verify   # a comment',
			'',
			'cargo fmt',
			'```',
			'```text',
			'not a command',
			'```',
			'```sh',
			'typos',
			'```'
		].join('\n');

		expect(shellLines(markdown)).toEqual(['pnpm verify   # a comment', 'cargo fmt', 'typos']);
	});
});
