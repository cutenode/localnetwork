// `pnpm policy`: checks the repository against the conventions in .github/NEW_PROJECT.md. OPA checks
// the policies' format and runs their tests (*_test.rego), which have to cover every line, Regal
// lints them, and conftest runs them (conftest.toml) over the config files and the inputs
// inputs.ts writes. They run from pinned images, so this needs Docker, as CI's repo-checks.yml does.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { configFiles, writeInputs } from './inputs.ts';

const conftest =
	'openpolicyagent/conftest:v0.71.1@sha256:0e77cdf7c1fcde035f3c579e4aaa6664ee2a8be5777eaa9af9f520cdd02280c0';

const opa = 'openpolicyagent/opa:1.21.2@sha256:7459fa69661242c8983d6e4116779550cbb17608f1110ddcd4e9e24da4dcb0e5';

const regal =
	'ghcr.io/open-policy-agent/regal:0.43.0@sha256:29460f0ec1340d37f6c0c74bcbd88dd232fd8382c7a2337f0684415bd4c46da1';

const scratch = mkdtempSync(join(tmpdir(), 'policy-'));

try {
	const inputs = (await writeInputs(scratch)).map((name) => `/scratch/${name}`);

	const steps: { args: string[]; quiet?: boolean }[] = [
		{ args: [opa, 'fmt', '--list', '--fail', 'tools/policy'] },
		// Regal, OPA's linter, with its exceptions in .regal/config.yaml.
		{ args: [regal, 'lint', 'tools/policy'] },
		{ args: [opa, 'check', '--strict', 'tools/policy'] },
		{ args: [opa, 'test', 'tools/policy'] },
		// A pass prints the whole coverage report, and a miss only the total, so its output is left
		// out: `opa test --coverage tools/policy` names the lines no test reaches.
		{ args: [opa, 'test', '--threshold', '100', 'tools/policy'], quiet: true },
		{ args: [conftest, 'test', ...configFiles(), ...inputs] }
	];

	for (const { args, quiet } of steps) {
		const { status } = spawnSync(
			'docker',
			['run', '--rm', '-v', `${process.cwd()}:/repo:ro`, '-v', `${scratch}:/scratch:ro`, '-w', '/repo', ...args],
			{ stdio: ['inherit', quiet ? 'ignore' : 'inherit', 'inherit'] }
		);

		if (status !== 0) {
			process.exitCode = 1;
			break;
		}
	}
} finally {
	rmSync(scratch, { recursive: true, force: true });
}
