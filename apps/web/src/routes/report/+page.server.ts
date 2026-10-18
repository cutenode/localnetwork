import { fail, redirect } from '@sveltejs/kit';
import { githubIssueUrl, REPORT_LIMITS, type ReportFacts, reportBody } from '#lib/bug-report.ts';
import { SCOPES } from '#lib/server/atproto/oauth.ts';
import { browserLabel } from '#lib/server/services/bug-reports.ts';
import { requireViewer, safeNext, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/** The page the report is about (the footer link passes it as `from`), and the browser it came from. */
const factsOf = (request: Request, from: string | null): ReportFacts => ({
	page: safeNext(from, '') || undefined,
	browser: browserLabel(request.headers.get('user-agent'))
});

export const load: PageServerLoad = async ({ locals, request, url }) => {
	const facts = factsOf(request, url.searchParams.get('from'));
	const github = githubIssueUrl(facts);
	const { app, viewer } = locals;

	// Signed out, or with no space to post to, a report goes to GitHub.
	if (!viewer || !app.bugReports.enabled) redirect(303, github, { external: true });

	return { github, facts, canPost: await app.bugReports.canPost(viewer.did) };
};

export const actions: Actions = {
	/** Sends the viewer through sign-in to approve posting to userinput.app, then back here. */
	allow: async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		const from = url.searchParams.get('from');
		const next = `/report${from ? `?from=${encodeURIComponent(from)}` : ''}`;
		let to: URL;

		try {
			to = await locals.app.oauth.authorize(viewer.did, JSON.stringify({ next }), SCOPES.bugReport);
		} catch (error) {
			locals.log.info({ err: error }, 'could not ask to post bug reports');

			return fail(502, { error: "We couldn't reach your account's server. Try again in a minute." });
		}

		redirect(303, to, { external: true });
	},

	send: async ({ locals, request, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		const values = { title: text(form, 'title', REPORT_LIMITS.title), body: text(form, 'body', REPORT_LIMITS.body) };
		const facts = factsOf(request, url.searchParams.get('from'));

		if (!values.title) return fail(400, { error: 'Add a title.', values, github: githubIssueUrl(facts, values) });

		try {
			const posted = await locals.app.bugReports.post(viewer.did, {
				title: values.title,
				body: reportBody(values.body, facts)
			});

			return { posted };
		} catch (cause) {
			locals.log.warn({ err: cause }, 'could not post a bug report');

			return fail(502, { error: "We couldn't post your report.", values, github: githubIssueUrl(facts, values) });
		}
	}
};
