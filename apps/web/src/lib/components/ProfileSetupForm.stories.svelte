<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect } from 'storybook/test';
	import type { OnboardingData } from '#lib/onboarding/checklist.ts';
	import { submit } from '#lib/testing/submits.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import ProfileSetupForm from './ProfileSetupForm.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Profile setup form', component: ProfileSetupForm, tags: ['autodocs'] });

	/** A new account, before its first profile: `prefill` is what its Bluesky profile had. */
	const fresh = (prefill: OnboardingData['prefill']): OnboardingData => ({ profile: null, prefill, listing: false, follows: false });
</script>

<Story name="From Bluesky" args={{ data: fresh({ name: 'Pat', hasAvatar: true }) }} />

<Story name="No Bluesky picture" args={{ data: fresh({ name: 'Pat', hasAvatar: false }) }} />

<Story name="Nothing to start from" args={{ data: fresh(null) }} />

<Story name="Phone" args={{ data: fresh({ name: 'Pat', hasAvatar: true }) }} globals={phone} />

<Story
	name="Submitted"
	args={{ data: fresh({ name: 'Pat', hasAvatar: true }) }}
	play={async ({ canvasElement }) => {
		await expect(await submit(canvasElement, 'Create profile', 'name', 'blueskyAvatar')).toEqual({ name: ['Pat'], blueskyAvatar: ['on'] });
	}}
/>
