// The tools the root's verify scripts run that `pnpm install` doesn't bring, and how to install
// each, as .github/CONTRIBUTING.md's Checks has them. Each script checks for its own before its
// first step (run.ts), so a missing one fails the run up front instead of after the long builds.
import { spawnSync } from 'node:child_process';

const installs = {
	go: 'brew install go',
	lychee: 'brew install lychee',
	dist: 'cargo binstall cargo-dist@0.33.0'
};

type Tool = keyof typeof installs;

/** The tools each script needs. */
const needs = new Map<string, Tool[]>([
	['verify:rust', ['dist']],
	['verify:repo', ['go', 'lychee']]
]);

// Started rather than looked up on the PATH, so a Windows .exe counts too.
const found = (tool: Tool, env: NodeJS.ProcessEnv): boolean =>
	spawnSync(tool, ['--version'], { env, stdio: 'ignore' }).error === undefined;

/** A line for each tool `script` needs that isn't on the PATH, with how to install it. */
export const missing = (script: string, env = process.env): string[] =>
	(needs.get(script) ?? [])
		.filter((tool) => !found(tool, env))
		.map((tool) => `${tool} is missing, which ${script} needs: ${installs[tool]}`);
