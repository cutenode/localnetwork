/** userinput.app's record types: a space (a feedback board) and a post in one. */
export const USERINPUT = { space: 'app.userinput.space', discussion: 'app.userinput.discussion' } as const;

/**
 * The longest title and description the form takes, in UTF-16 code units as `maxlength` counts them. A post
 * takes 600 and 20,000 bytes, and a code unit is at most 3 bytes, so a report the form takes always fits,
 * with room in the description for the page and browser lines added under it.
 */
export const REPORT_LIMITS = { title: 200, body: 6_000 } as const;

/** GitHub refuses very long URLs, so the text prefilled into its form is cut to this. */
const PREFILL_MAX = 2_000;

/** What the site knows about where a report comes from: the page and the browser. */
export type ReportFacts = { page?: string; browser?: string };

/** GitHub's form for a website bug (.github/ISSUE_TEMPLATE/web-bug.yml), with what's known filled in. */
export function githubIssueUrl(facts: ReportFacts, typed: { title?: string; body?: string } = {}): string {
	const url = new URL('https://github.com/cutenode/localnetwork/issues/new');

	const fields = {
		template: 'web-bug.yml',
		title: typed.title,
		'what-happened': typed.body?.slice(0, PREFILL_MAX),
		steps: facts.page && `1. Open ${facts.page}\n2. `,
		browser: facts.browser
	};

	for (const [id, value] of Object.entries(fields)) if (value) url.searchParams.set(id, value);

	return url.href;
}

/** A post's description: what they wrote, then the page and browser, as the GitHub form asks for them. */
export function reportBody(text: string, facts: ReportFacts): string {
	const lines = [facts.page && `Page: ${facts.page}`, facts.browser && `Browser: ${facts.browser}`].filter(Boolean);

	return lines.length ? `${text}\n\n${lines.join('\n')}` : text;
}

/** Where userinput.app shows a post, from the post's at:// URI. */
export function discussionUrl(uri: string): string {
	const [did, , rkey] = uri.replace('at://', '').split('/');

	return `https://userinput.app/d/${did}/${rkey}`;
}
