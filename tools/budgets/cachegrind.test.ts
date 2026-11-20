import { execFileSync } from 'node:child_process';
import { describe, expect, it } from 'vitest';

const hasValgrind = (() => {
	try {
		execFileSync('valgrind', ['--version'], { stdio: 'ignore' });

		return true;
	} catch {
		return false;
	}
})();

// `env` prints its environment, so what it runs grows with the environment's size. Where valgrind runs:
// Linux, as in CI's tools tests, not macOS.
describe.runIf(hasValgrind)('cachegrind.sh', () => {
	const counted = (pad: string, ...args: string[]) =>
		execFileSync('tools/budgets/cachegrind.sh', args, {
			encoding: 'utf8',
			env: { ...process.env, PAD: pad },
			stdio: ['ignore', 'pipe', 'ignore']
		}).trim();

	it("counts the same whatever environment it's run from", () => {
		expect(counted('', '/usr/bin/env')).toMatch(/^\d+$/);
		expect(counted('x'.repeat(10), '/usr/bin/env')).toBe(counted('', '/usr/bin/env'));
	});
	it('passes on the variables given', () => {
		expect(Number(counted('', 'A=b', '/usr/bin/env'))).toBeGreaterThan(Number(counted('', '/usr/bin/env')));
	});
});
