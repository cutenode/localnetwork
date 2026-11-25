// What ci.yml's scope step decides about macOS and Windows, from the step's own script.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { parse } from 'yaml';

// SAFETY: ci.yml is the repository's own workflow, and tools/policy/workflows.rego checks its shape.
const ci = parse(readFileSync('.github/workflows/ci.yml', 'utf8')) as {
	jobs: { changes: { steps: { id?: string; run?: string }[] } };
};

const script = ci.jobs.changes.steps.find(({ id }) => id === 'scope')?.run ?? '';

// The step's associative arrays need bash 4, and macOS's own is 3.2.
const bash = Number(execFileSync('bash', ['-c', 'echo "${BASH_VERSINFO[0]}"'], { encoding: 'utf8' }));

describe.skipIf(bash < 4)('the scope step', () => {
	let dir: string;

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'scope-'));
	});
	afterEach(() => {
		rmSync(dir, { recursive: true });
	});

	/** Which of locallibrary's OSes the step runs, and `called` when it calls its workflow, when `filter` matched. */
	const decided = (env: Record<string, string>, filter = 'locallibrary') => {
		const output = join(dir, 'output');

		execFileSync('bash', ['-eo', 'pipefail', '-c', script], {
			env: {
				...process.env,
				GITHUB_OUTPUT: output,
				CHANGED_FILES: '0',
				FILTER: JSON.stringify({ [filter]: 'true' }),
				DRAFT: '',
				ALL_OS: '',
				BASE: 'main',
				LAST_PASSED: '',
				WINDOWS_TESTED: '',
				...env
			}
		});
		const on = readFileSync(output, 'utf8').split('\n');

		return ['windows', 'macos']
			.filter((os) => on.includes(`locallibrary_${os}=true`))
			.concat(on.includes('locallibrary=true') ? ['called'] : []);
	};

	it('runs Windows on a pull request, and macOS too with the all-os label', () => {
		expect(decided({ EVENT: 'pull_request' })).toEqual(['windows', 'called']);
		expect(decided({ EVENT: 'pull_request', ALL_OS: 'true' })).toEqual(['windows', 'macos', 'called']);
	});

	it("runs Windows on a pull request whatever a group's check says", () => {
		expect(decided({ EVENT: 'pull_request', WINDOWS_TESTED: 'true' })).toEqual(['windows', 'called']);
	});

	it('runs neither on a draft', () => {
		expect(decided({ EVENT: 'pull_request', DRAFT: 'true', ALL_OS: 'true' })).toEqual([]);
	});

	it('runs macOS alone on a group whose pull request passed Windows on the same files', () => {
		expect(decided({ EVENT: 'merge_group', WINDOWS_TESTED: 'true' })).toEqual(['macos', 'called']);
	});

	it.each(['false', '', 'TRUE'])('runs both on a group when the check says %j', (WINDOWS_TESTED) => {
		expect(decided({ EVENT: 'merge_group', WINDOWS_TESTED })).toEqual(['windows', 'macos', 'called']);
	});

	it('runs neither on a group that changes none of their code, or a push', () => {
		expect(decided({ EVENT: 'merge_group' }, 'web')).toEqual([]);
		expect(decided({ EVENT: 'push' })).toEqual([]);
	});
});
