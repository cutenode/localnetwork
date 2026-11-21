import { fail, redirect } from '@sveltejs/kit';
import { githubIssueUrl, REPORT_LIMITS, type ReportFacts, reportBody } from '#lib/bug-report.ts';
import { OptionalScopeUnsupported } from '#lib/server/atproto/oauth.ts';
import { browserLabel } from '#lib/server/services/bug-reports.ts';
import { requireViewer, safeNext, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/** The page the report is about (the footer link passes it as `from`), and the browser it came from. */
const factsOf = (request: Request, from: string | null): ReportFacts => ({
	page: safeNext(from, '') || undefined,
	browser: browserLabel(request.headers.get('user-agent'))
});

/**
 * Whether a signed-in viewer's report goes to userinput.app: the site has a space to post to, and it's rolled out to
 * them. Off, it goes to GitHub, which is also how staff send everyone there while userinput.app is down.
 */
const postsToUserinput = ({ app, flags }: App.Locals) => app.bugReports.enabled && flags.userinputReports;

export const load: PageServerLoad = async ({ locals, request, url }) => {
	const facts = factsOf(request, url.searchParams.get('from'));
	const github = githubIssueUrl(facts);
	const { app, viewer } = locals;

	// Signed out, or with reports to userinput.app off for them, a report goes to GitHub.
	if (!viewer || !postsToUserinput(locals)) redirect(303, github, { external: true });

	return { github, facts, canPost: await app.bugReports.canPost(viewer.did) };
};

export const actions: Actions = {
	/** Sends the viewer through sign-in to approve posting to userinput.app, then back here. */
	allow: async ({ locals, request, url }) => {
		const viewer = requireViewer(locals, url);
		const from = url.searchParams.get('from');

		if (!postsToUserinput(locals)) redirect(303, githubIssueUrl(factsOf(request, from)), { external: true });

		const next = `/report${from ? `?from=${encodeURIComponent(from)}` : ''}`;
		let to: URL;
		const account = await locals.app.accounts.get(viewer.did);

		try {
			to = await locals.app.oauth.authorize(viewer.did, JSON.stringify({ next }), { account, optional: 'bugReport' });
		} catch (error) {
			locals.log.info('could not ask to post bug reports', { err: error });

			if (error instanceof OptionalScopeUnsupported) {
				return fail(400, { error: "Your account's server doesn't let apps post bug reports for you." });
			}

			return fail(502, { error: "We couldn't reach your account's server. Try again in a minute." });
		}

		redirect(303, to, { external: true });
	},

	send: async ({ locals, request, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		const values = { title: text(form, 'title', REPORT_LIMITS.title), body: text(form, 'body', REPORT_LIMITS.body) };
		const facts = factsOf(request, url.searchParams.get('from'));

		// What they wrote goes with them.
		if (!postsToUserinput(locals)) redirect(303, githubIssueUrl(facts, values), { external: true });

		if (!values.title) return fail(400, { error: 'Add a title.', values, github: githubIssueUrl(facts, values) });

		try {
			const posted = await locals.app.bugReports.post(viewer.did, {
				title: values.title,
				body: reportBody(values.body, facts)
			});

			return { posted };
		} catch (cause) {
			locals.log.warn('could not post a bug report', { err: cause });

			return fail(502, { error: "We couldn't post your report.", values, github: githubIssueUrl(facts, values) });
		}
	}
};
