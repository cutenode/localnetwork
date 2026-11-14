import { nextNights, soon, withOthers } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import { locallibraryStatus } from '#lib/server/locallibrary.ts';
import { onboardingData } from '#lib/server/onboarding.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;
	const { projects, sections } = await parent();

	const threads = app.bookings.threadsFor(viewer.did);

	// Undefined while the directory can't say, so nobody is asked to set up a profile they have.
	const profile = app.profiles.get(viewer.did).catch((error) => {
		locals.log.warn("couldn't tell whether the account has a profile", { err: error });

		return undefined;
	});

	// Someone without a profile makes one from the checklist, filled in from their Bluesky profile.
	const firstRun = profile.then(async (found) => {
		if (found !== null) return null;
		const prefill = (await app.profiles.blueskyPrefill(viewer.did)) ?? {};

		return { name: prefill.name ?? viewer.handle ?? '', hasAvatar: !!prefill.avatar };
	});

	const summary = Promise.all([threads, app.requests.list({ requester: viewer.did, status: 'pending' })]).then(
		([all, requests]) => ({ others: withOthers(all, requests, viewer.did), hasBookings: all.length > 0 })
	);

	// Each streams on its own if it isn't ready at once: shows come from the directory, plans from the account's own
	// server, and neither holds up the rest.
	const nights = threads.then(async (all) => nextNights(locals, viewer, all, await projects, await sections));

	const [setup, said, next, steps, published] = await Promise.all([
		soon(firstRun),
		soon(summary),
		soon(nights),
		soon(onboardingData(locals, viewer, profile, firstRun)),
		soon(locallibraryStatus(locals, viewer))
	]);

	return {
		firstRun: setup.value,
		summary: said.value,
		nights: next.value,
		onboarding: steps.value,
		locallibrary: published.value
	};
};
