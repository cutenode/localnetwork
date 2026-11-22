import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { configFiles, shellLines, writeInputs } from './inputs.ts';

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
			'harden.json',
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
