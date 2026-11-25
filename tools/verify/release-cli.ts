// `node tools/verify/release-cli.ts`: dist's plan, as locallibrary-release.yml's plan job makes it,
// checked by release.ts.
import { spawnSync } from 'node:child_process';
import { releaseProblems } from './release.ts';

const { status, stdout } = spawnSync('dist', ['plan', '--output-format=json'], {
	encoding: 'utf8',
	stdio: ['inherit', 'pipe', 'inherit']
});

const problems = status === 0 ? releaseProblems(stdout) : ['dist plan failed'];

for (const line of problems) console.error(line);

process.exitCode = problems.length > 0 ? 1 : 0;
