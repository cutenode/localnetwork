// `node tools/queue/cli.ts`, on a merge queue group: prints `tested=true` or `tested=false` for the
// step's outputs, and why to the log. windows.ts says what it compares.
import { execFileSync } from 'node:child_process';
import { windowsTested } from './windows.ts';

const { GITHUB_REPOSITORY: repo = '', GITHUB_SHA: sha = '', WORKFLOWS_CHANGED: workflowsChanged } = process.env;

const verdict = windowsTested({ sha, repo, workflowsChanged }, (path) =>
	execFileSync('gh', ['api', `repos/${repo}/${path}`], { encoding: 'utf8', maxBuffer: 1 << 28 })
);

console.error(
	verdict.tested
		? `::notice title=Windows skipped::${verdict.run} passed it on the same files`
		: `::notice title=Windows runs::${verdict.why}`
);

process.stdout.write(`tested=${verdict.tested}\n`);
