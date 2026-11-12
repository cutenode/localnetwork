import type { OnboardingData } from '#lib/onboarding/checklist.ts';
import type { ProfileView, Viewer } from '#lib/types.ts';

/**
 * What the dashboard home's checklist checks for `viewer`, from their `profile` and `prefill`, the start of a first
 * one. Null while the directory can't say whether they have a profile, so nobody is asked to make one they have.
 */
export async function onboardingData(
	{ app, log }: Pick<App.Locals, 'app' | 'log'>,
	viewer: Viewer,
	profile: Promise<ProfileView | null | undefined>,
	prefill: Promise<OnboardingData['prefill']>
): Promise<OnboardingData | null> {
	// Each answers one step, so one that can't be read leaves that step to do instead of hiding the checklist.
	const orUndone = (what: string, check: Promise<boolean>) =>
		check.catch((error) => {
			log.warn(`couldn't tell whether the account ${what}`, { err: error });

			return false;
		});

	const requested = app.requests.list({ kind: 'list', requester: viewer.did, status: 'open', limit: 1 });

	const [found, start, listing, follows] = await Promise.all([
		profile,
		prefill,
		orUndone(
			'is listed in Find DJs',
			Promise.all([app.djListings.get(viewer.did), requested]).then(([listed, [asked]]) => !!(listed ?? asked))
		),
		orUndone('follows anyone', app.follows.followsAny(viewer.did))
	]);

	if (found === undefined) return null;

	return { profile: found?.record ?? null, prefill: start, listing, follows };
}
