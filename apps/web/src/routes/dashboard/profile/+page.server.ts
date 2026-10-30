import { redirect } from '@sveltejs/kit';
import { grantsProfile } from '#lib/server/atproto/oauth.ts';
import { formValues } from '#lib/server/forms/project.ts';
import { parseProfileForm, profileFormValues, sectionsOf } from '#lib/server/forms/profile.ts';
import { act, limit, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;

	const [profile, account, listing, [listRequest]] = await Promise.all([
		app.profiles.get(viewer.did),
		app.accounts.get(viewer.did),
		app.djListings.get(viewer.did),
		app.requests.list({ kind: 'list', requester: viewer.did, status: 'open', limit: 1 })
	]);

	// Someone without a profile starts from their Bluesky one.
	const prefill = profile ? null : await app.profiles.blueskyPrefill(viewer.did);

	return {
		profile,
		values: profile ? profileFormValues(profile.record) : { name: prefill?.name, bio: prefill?.bio },
		sectionsOn: sectionsOf(profile?.record),
		// Whether staff list them in Find DJs, or are looking at their request to be.
		listing: listing ? { listedAt: listing.createdAt } : null,
		listRequest: listRequest ? { rkey: listRequest.rkey, status: listRequest.status } : null,
		// A session from before profiles can't save one, so it signs in again before filling the form in, not after.
		needsSignIn: !grantsProfile(account?.grantedScope, viewer.did),
		geocoder: app.geocoder.enabled,
		// Streamed, so the form shows while HappyView reads their sets.
		sets: app.sets.published([viewer.did])
	};
};

export const actions: Actions = {
	save: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		const { app } = locals;

		const result = await act(async () => {
			const swap = text(form, 'cid', 200) || null;
			const previous = await app.profiles.get(viewer.did);
			await app.profiles.save(viewer, { ...(await parseProfileForm(form, previous?.record)), swap });

			return { saved: true };
		}, formValues(form));

		if ('saved' in result) redirect(303, '/dashboard/profile?saved=1');

		return result;
	},

	/** Asks staff to list the account's profile in Find DJs. */
	requestListing: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		const { app } = locals;

		const result = await act(async () => {
			const profile = await app.profiles.get(viewer.did);

			const submitted = await app.requests.submit({
				actor: viewer,
				kind: 'list',
				listing: profile ? { subject: profile.uri, cid: profile.cid } : undefined,
				message: text(form, 'message', 20_000) || undefined
			});

			return { rkey: submitted.rkey };
		});

		if ('rkey' in result) redirect(303, `/dashboard/requests/${result.rkey}?submitted=1`);

		return result;
	},

	/** The dashboard's first-run card: a profile from a name, and the Bluesky picture when asked for. */
	create: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		const { app } = locals;

		const result = await act(async () => {
			const name = limit('Name', text(form, 'name', 640), 64) || undefined;
			const prefill = await app.profiles.blueskyPrefill(viewer.did);
			const avatar = form.get('blueskyAvatar') === 'on' && prefill?.avatar ? { blob: prefill.avatar } : 'remove';

			await app.profiles.save(viewer, {
				basics: {
					name: name ?? prefill?.name ?? viewer.handle ?? viewer.did,
					...(prefill?.bio && { bio: prefill.bio })
				},
				avatar,
				swap: null
			});

			return { created: true };
		});

		if ('created' in result) redirect(303, '/dashboard/profile?created=1');

		return result;
	}
};
