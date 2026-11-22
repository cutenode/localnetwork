import { describe, expect, it } from 'vitest';
import { crateSteps, verifyCrate } from './crate.ts';
import { path } from './fakes.ts';

describe('crateSteps', () => {
	it("runs each of cargo's steps on the crate alone", () => {
		const cargo = crateSteps('locallibrary-plugin-discogs').filter((step) => /^cargo (fmt|clippy|nextest)/.test(step));

		expect(cargo).toHaveLength(3);

		for (const step of cargo) expect(step).toContain(' -p locallibrary-plugin-discogs ');
	});

	it('adds the workspace-wide checks that build nothing', () => {
		expect(crateSteps('x').slice(3)).toEqual(['cargo shear', 'pnpm policy']);
	});
});

describe('verifyCrate', () => {
	it("runs the crate's steps, then lists what the full verify:rust adds", () => {
		// A stand-in for the crate's steps, which would build it.
		const { lines, ok } = verifyCrate('x', path({ cargo: '', docker: '' }), ['exit 0']);

		expect(ok).toBe(true);
		expect(lines).toContain('verify:rust --package: every step passed');
		expect(lines).toContain('The full `pnpm verify:rust` also runs:');
		expect(lines).toContain('  cargo clippy --workspace --all-targets --locked -- -D warnings');
		expect(lines).not.toContain('  cargo shear');
	});

	it('needs Docker for the policies', () => {
		expect(verifyCrate('x', path({ cargo: '' })).lines[0]).toBe(
			'verify:rust --package needs Docker: start Docker or OrbStack, which `pnpm policy` runs in'
		);
	});

	it("refuses a name that isn't a crate's", () => {
		expect(verifyCrate('x; rm -rf ~')).toEqual({ lines: ["x; rm -rf ~ isn't a crate name"], ok: false });
	});
});
