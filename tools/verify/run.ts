// The root's verify scripts: each checks what it needs (tools.ts), then runs its steps in steps.json
// one after another from the repository root, going on past a failure, and ends with a line for each
// step saying whether it passed, failed or was skipped, how long it took, and the command that reruns
// it alone, then what the run left unchecked. tools/policy/apps.rego holds the steps to CI's
// (inputs.ts gives conftest the file).
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { preflight } from './tools.ts';

export interface Verdict {
	lines: string[];
	ok: boolean;
}

interface Outcome {
	command: string;
	status: 'passed' | 'failed' | 'skipped';
	seconds: number;
}

/** Each script's steps, from steps.json. */
export const steps = new Map(
	// SAFETY: steps.json maps each script to its commands, and tools/policy/apps.rego reads it as that.
	Object.entries(JSON.parse(readFileSync(new URL('steps.json', import.meta.url), 'utf8')) as Record<string, string[]>)
);

// With the environment the tool check saw, so a step runs the tools it found.
const step = (command: string, env: NodeJS.ProcessEnv): Outcome => {
	console.log(`\n> ${command}`);

	const start = performance.now();
	const { status } = spawnSync(command, { shell: true, stdio: 'inherit', env });

	return { command, status: status === 0 ? 'passed' : 'failed', seconds: (performance.now() - start) / 1000 };
};

const line = ({ command, status, seconds }: Outcome): string =>
	`  ${status.padEnd(7)} ${seconds.toFixed(1).padStart(6)}s  ${command}`;

/** Runs `script`'s steps, once every tool it needs is on `env`'s PATH. */
export const verify = (script: string, from = steps, env = process.env): Verdict => {
	const commands = from.get(script);

	if (commands === undefined) return { lines: [`tools/verify/steps.json has no ${script}`], ok: false };

	const { problems, notes, skipped } = preflight(script, env);

	if (problems.length > 0) return { lines: problems, ok: false };

	const outcomes = commands.map((command) =>
		skipped.includes(command) ? { command, status: 'skipped' as const, seconds: 0 } : step(command, env)
	);

	const count = (status: Outcome['status']) => outcomes.filter((outcome) => outcome.status === status).length;
	const [failed, left] = [count('failed'), count('skipped')];

	const verdict =
		failed > 0
			? `${failed} of ${outcomes.length} steps failed`
			: left > 0
				? `${outcomes.length - left} passed, ${left} skipped`
				: 'every step passed';

	return {
		lines: ['', `${script}: ${verdict}`, ...outcomes.map(line), ...notes.map((note) => `note: ${note}`)],
		ok: failed === 0
	};
};
