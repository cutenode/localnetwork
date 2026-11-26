import { describe, expect, it } from 'vitest';
import { type Commit, type Get, type Group, type Job, type Run, windowsTested } from './windows.ts';

const REPO = 'cutenode/localnetwork';

const GROUP: Group = { sha: 'group', repo: REPO, workflowsChanged: 'false' };

const RUN = 'https://github.com/cutenode/localnetwork/actions/runs/7';

const job = (name: string, label: string, conclusion: string | null = 'success'): Job => ({
	name,
	labels: [label],
	conclusion
});

const called = (sha = 'merge', ref = 'refs/pull/762/merge') => ({ sha, ref });

const pullRequestRun: Run = {
	id: 7,
	event: 'pull_request',
	head_sha: 'head',
	conclusion: 'success',
	html_url: RUN,
	head_repository: { full_name: REPO },
	referenced_workflows: [called(), called()]
};

const commit = (tree: string, ...parents: string[]): Commit => ({
	tree: { sha: tree },
	parents: parents.map((sha) => ({ sha }))
});

const GROUP_COMMIT = 'git/commits/group';

const TESTED_COMMIT = 'git/commits/merge';

const RUNS = 'actions/workflows/ci.yml/runs?per_page=100';

const JOBS = 'actions/runs/7/jobs?per_page=100';

type Answer = Commit | { workflow_runs?: Run[] } | { total_count: number; jobs: Job[] };

/** GitHub with a group that merges `head`, whose pull request's run passed Windows on the same tree, but for `answer` to `path`. */
const github = (path?: string, answer?: Answer): Get => {
	const answers = new Map<string, Answer | undefined>([
		[GROUP_COMMIT, commit('tree', 'main', 'head')],
		[TESTED_COMMIT, commit('tree', 'main', 'head')],
		[RUNS, { workflow_runs: [pullRequestRun] }],
		[
			JOBS,
			{
				total_count: 3,
				jobs: [
					job('changes', 'ubuntu-latest'),
					job('other-os (windows-latest, 1)', 'windows-latest'),
					job('other-os (windows-latest, lint)', 'windows-latest')
				]
			}
		]
	]);

	if (path !== undefined) answers.set(path, answer);

	return (asked) => {
		const found = answers.get(asked);

		if (found === undefined) throw new Error(`HTTP 404 ${asked}`);

		return JSON.stringify(found);
	};
};

const runs = (...listed: Run[]) => github(RUNS, { workflow_runs: listed });

const jobs = (total: number, ...listed: Job[]) => github(JOBS, { total_count: total, jobs: listed });

const why = (get: Get, group = GROUP) => {
	const verdict = windowsTested(group, get);

	return verdict.tested ? `tested by ${verdict.run}` : verdict.why;
};

describe('windowsTested', () => {
	it("takes the pull request's run that passed Windows on the group's tree", () => {
		expect(windowsTested(GROUP, github())).toEqual({ tested: true, run: RUN });
	});

	it('passes over runs of other events and commits', () => {
		const others = [
			{ ...pullRequestRun, id: 9, event: 'merge_group' },
			{ ...pullRequestRun, id: 8, head_sha: 'another' }
		];

		expect(windowsTested(GROUP, runs(...others, pullRequestRun)).tested).toBe(true);
	});

	it.each(['true', undefined, ''])('runs Windows when the workflows changed is %j', (workflowsChanged) => {
		expect(why(github(), { ...GROUP, workflowsChanged })).toBe('the group changes a workflow or an action');
	});

	it.each([
		['one parent', commit('tree', 'main')],
		['three parents', commit('tree', 'main', 'head', 'other')]
	])("runs Windows when the group's commit has %s", (_, group) => {
		expect(why(github(GROUP_COMMIT, group))).toBe("the group's commit doesn't merge one pull request");
	});

	it('runs Windows when no run is for the head', () => {
		expect(why(runs())).toBe("none of CI's last 100 runs is a pull request's on head");
	});

	it.each([[{ full_name: 'someone/localnetwork' }], [null]])('runs Windows for a run from %j', (head_repository) => {
		expect(why(runs({ ...pullRequestRun, head_repository }))).toBe(`${RUN} is from a fork`);
	});

	it.each(['failure', 'cancelled', null])(
		'runs Windows when the newest run is %s, past one that passed',
		(conclusion) => {
			expect(why(runs({ ...pullRequestRun, conclusion }, { ...pullRequestRun, id: 6 }))).toBe(`${RUN} hasn't passed`);
		}
	);

	it.each([
		['no workflows', []],
		['no list of them', undefined],
		['two commits', [called(), called('other')]],
		['another ref', [called(), called('merge', 'refs/heads/main')]],
		['no ref', [{ sha: 'merge' }]]
	])('runs Windows when GitHub names %s', (_, referenced_workflows) => {
		expect(why(runs({ ...pullRequestRun, referenced_workflows }))).toBe(
			`GitHub doesn't name one merge commit ${RUN} tested`
		);
	});

	it('runs Windows when the run tested another tree', () => {
		expect(why(github(TESTED_COMMIT, commit('stale', 'main', 'head')))).toBe(
			`${RUN} tested other files: stale, not tree`
		);
	});

	it('runs Windows when the jobs are more than one page', () => {
		expect(why(jobs(101, job('other-os (windows-latest, 1)', 'windows-latest')))).toBe(`${RUN} has more than 100 jobs`);
	});

	it('runs Windows when the run had no Windows job, only a Linux one named for it', () => {
		expect(why(jobs(1, job('clippy (Windows, cross)', 'ubuntu-latest')))).toBe(`${RUN} ran nothing on Windows`);
	});

	it.each(['failure', 'skipped', 'cancelled', null])('runs Windows when one of its jobs is %s', (conclusion) => {
		const passed = job('other-os (windows-latest, 1)', 'windows-latest');

		expect(why(jobs(2, passed, job('other-os (windows-latest, 2)', 'windows-latest', conclusion)))).toBe(
			`${RUN} didn't pass other-os (windows-latest, 2)`
		);
	});

	it.each([
		['a request fails', github(TESTED_COMMIT)],
		['an answer is of another shape', github(RUNS, {})]
	])('runs Windows when %s', (_, get) => {
		expect(why(get)).toMatch(/^asking GitHub failed: /);
	});
});
