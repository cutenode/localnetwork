import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { verify } from './run.ts';

const dir = mkdtempSync(join(tmpdir(), 'verify-run-'));

afterAll(() => rmSync(dir, { recursive: true, force: true }));

const marker = join(dir, 'ran');

describe('verify', () => {
	it('runs every step past a failure, and names the failure with its time', () => {
		const { lines, ok } = verify('demo', new Map([['demo', ['exit 3', `echo > "${marker}"`]]]));

		expect(ok).toBe(false);
		expect(existsSync(marker)).toBe(true);
		expect(lines.slice(1, 2)).toEqual(['demo: 1 of 2 steps failed']);
		expect(lines[2]).toMatch(/^ {2}failed +\d+\.\ds {2}exit 3$/);
		expect(lines[3]).toMatch(/^ {2}passed +\d+\.\ds {2}echo > /);
	});

	it('passes when every step does', () => {
		expect(verify('demo', new Map([['demo', ['exit 0']]]))).toEqual({
			lines: ['', 'demo: every step passed', expect.stringMatching(/^ {2}passed +\d+\.\ds {2}exit 0$/)],
			ok: true
		});
	});

	it('runs nothing while a tool the script needs is missing', () => {
		rmSync(marker, { force: true });

		const { lines, ok } = verify('verify:repo', new Map([['verify:repo', [`echo > "${marker}"`]]]), { PATH: '' });

		expect(ok).toBe(false);
		expect(lines).toContain('lychee is missing, which verify:repo needs: brew install lychee');
		expect(existsSync(marker)).toBe(false);
	});

	it('runs the steps with the environment the tool check saw', () => {
		expect(
			verify('demo', new Map([['demo', ['test "$VERIFY_RUN" = given']]]), { ...process.env, VERIFY_RUN: 'given' }).ok
		).toBe(true);
	});

	it('fails a script steps.json lacks', () => {
		expect(verify('verify:nothing')).toEqual({ lines: ['tools/verify/steps.json has no verify:nothing'], ok: false });
	});
});

describe('steps.json', () => {
	const read = (path: string): object => JSON.parse(readFileSync(join(import.meta.dirname, path), 'utf8'));

	it('has a script in package.json that runs each list', () => {
		const scripts = Object.keys(read('steps.json'));

		expect(read('../../package.json')).toMatchObject({
			scripts: Object.fromEntries(scripts.map((script) => [script, `node tools/verify/cli.ts ${script}`]))
		});
	});
});
