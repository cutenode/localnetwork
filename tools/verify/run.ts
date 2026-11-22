// The root's verify scripts: each runs its steps in steps.json one after another from the
// repository root, going on past a failure, and ends with a line for each step saying whether it
// passed, how long it took, and the command that reruns it alone. tools/policy/apps.rego holds the
// steps to CI's (inputs.ts gives conftest the file).
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { missing } from './tools.ts';

export interface Verdict {
	lines: string[];
	ok: boolean;
}

interface Outcome {
	command: string;
	passed: boolean;
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

	return { command, passed: status === 0, seconds: (performance.now() - start) / 1000 };
};

const line = ({ command, passed, seconds }: Outcome): string =>
	`  ${passed ? 'passed' : 'failed'} ${seconds.toFixed(1).padStart(7)}s  ${command}`;

/** Runs `script`'s steps, once every tool it needs is on the PATH. */
export const verify = (script: string, from = steps, env = process.env): Verdict => {
	const commands = from.get(script);

	if (commands === undefined) return { lines: [`tools/verify/steps.json has no ${script}`], ok: false };

	const absent = missing(script, env);

	if (absent.length > 0) return { lines: absent, ok: false };

	const outcomes = commands.map((command) => step(command, env));
	const failed = outcomes.filter(({ passed }) => !passed).length;
	const verdict = failed === 0 ? 'every step passed' : `${failed} of ${outcomes.length} steps failed`;

	return { lines: ['', `${script}: ${verdict}`, ...outcomes.map(line)], ok: failed === 0 };
};
