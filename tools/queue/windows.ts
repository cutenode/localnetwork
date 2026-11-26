// Whether a merge queue group can skip its Windows jobs: its pull request's own run passed them on
// the same files. ci.yml's changes job asks, for each group.
//
// What a run tested comes from GitHub, not from the run: a run's `referenced_workflows` name the
// commit its calls were read at, which for a pull request is the merge commit it tested. Nothing a
// pull request changes can write that. A group whose tree is that commit's holds the same files, the
// workflows included, so the same jobs would run the same tests. Only runs from a branch of this
// repository count, and a group that changes a workflow or an action never skips. The answer is yes
// only when all of it is there and agrees: anything missing or different, a request that fails
// included, runs Windows.

/** GitHub's answer to a GET of an API path under the repository, as JSON text. */
export type Get = (path: string) => string;

export type Group = {
	/** The group's commit: the pull request's head, merged into the commit the group goes on. */
	sha: string;
	/** `owner/name`. */
	repo: string;
	/** ci.yml's workflow_lint filter for the group: whether it changes a workflow or an action. */
	workflowsChanged: string | undefined;
};

export type Commit = { tree: { sha: string }; parents: { sha: string }[] };

export type Run = {
	id: number;
	event: string;
	head_sha: string;
	conclusion: string | null;
	html_url: string;
	head_repository: { full_name: string } | null;
	referenced_workflows?: { sha: string; ref?: string }[];
};

export type Job = { name: string; labels: string[]; conclusion: string | null };

const JOBS = 100;

/** Why the group's Windows jobs have to run, or the pull request's run that stands in for them. */
export type Verdict = { tested: false; why: string } | { tested: true; run: string };

const no = (why: string): Verdict => ({ tested: false, why });

/** The merge commit `run` tested, when every workflow it called was read at that one commit. */
function testedCommit({ referenced_workflows: called = [] }: Run): string | undefined {
	const [sha, ...others] = new Set(called.map((workflow) => workflow.sha));
	const merges = called.every(({ ref }) => /^refs\/pull\/\d+\/merge$/.test(ref ?? ''));

	return merges && others.length === 0 ? sha : undefined;
}

function compared({ sha, repo, workflowsChanged }: Group, get: Get): Verdict {
	// SAFETY: GitHub's git commit object.
	const commit = (of: string) => JSON.parse(get(`git/commits/${of}`)) as Commit;

	if (workflowsChanged !== 'false') return no('the group changes a workflow or an action');

	const group = commit(sha);
	const [, head, ...more] = group.parents;

	if (head === undefined || more.length > 0) return no("the group's commit doesn't merge one pull request");

	// Filtered here rather than with the list's parameters, which go through a search index that
	// sometimes finds nothing (see .github/actions/last-passed). Newest first.
	// SAFETY: GitHub's listing of a workflow's runs.
	const listed = JSON.parse(get('actions/workflows/ci.yml/runs?per_page=100')) as { workflow_runs: Run[] };
	const run = listed.workflow_runs.find((each) => each.event === 'pull_request' && each.head_sha === head.sha);

	if (run === undefined) return no(`none of CI's last 100 runs is a pull request's on ${head.sha}`);

	if (run.head_repository?.full_name !== repo) return no(`${run.html_url} is from a fork`);

	if (run.conclusion !== 'success') return no(`${run.html_url} hasn't passed`);

	const tested = testedCommit(run);

	if (tested === undefined) return no(`GitHub doesn't name one merge commit ${run.html_url} tested`);

	const { tree } = commit(tested);

	if (tree.sha !== group.tree.sha) return no(`${run.html_url} tested other files: ${tree.sha}, not ${group.tree.sha}`);

	// SAFETY: GitHub's listing of a run's jobs, of its latest attempt.
	const { total_count: total, jobs } = JSON.parse(get(`actions/runs/${run.id}/jobs?per_page=${JOBS}`)) as {
		total_count: number;
		jobs: Job[];
	};

	if (total > JOBS) return no(`${run.html_url} has more than ${JOBS} jobs`);

	const windows = jobs.filter(({ labels }) => labels.some((label) => label.startsWith('windows-')));
	const unpassed = windows.filter(({ conclusion }) => conclusion !== 'success');

	if (windows.length === 0) return no(`${run.html_url} ran nothing on Windows`);

	if (unpassed.length > 0) return no(`${run.html_url} didn't pass ${unpassed.map(({ name }) => name).join(', ')}`);

	return { tested: true, run: run.html_url };
}

/** Whether `group`'s pull request already passed its Windows jobs on the same files. */
export function windowsTested(group: Group, get: Get): Verdict {
	try {
		return compared(group, get);
	} catch (error) {
		return no(`asking GitHub failed: ${String(error)}`);
	}
}
