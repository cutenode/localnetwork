<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { RequesterAbout } from '#lib/epk.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import RequesterFacts from './RequesterFacts.svelte';

	const { Story } = defineMeta({ title: 'Press kits/Requester facts', component: RequesterFacts, tags: ['autodocs'] });

	const nobody: RequesterAbout = { promoter: null, performer: null, venues: [] };

	const promoter: RequesterAbout = {
		...nobody,
		promoter: { cities: [{ city: 'Brooklyn', region: 'New York', country: 'US' }, { city: 'Philadelphia', country: 'US' }] }
	};

	const venueOwner: RequesterAbout = {
		...nobody,
		venues: [
			{ name: 'Elsewhere', locality: 'Brooklyn', role: 'owner' },
			{ name: 'Nowadays', locality: 'Ridgewood', role: 'booker' }
		]
	};

	const everything: RequesterAbout = {
		promoter: promoter.promoter,
		performer: { genres: ['dubstep', 'riddim', 'baile funk', 'jungle'] },
		venues: venueOwner.venues
	};
</script>

<Story name="Promoter" args={{ about: promoter }} />

<Story name="Venue owner" args={{ about: venueOwner }} />

<Story name="Promoter, DJ and venues" args={{ about: everything }} />

<Story name="Promoter, DJ and venues on a phone" args={{ about: everything }} globals={phone} />

<Story name="Neither" args={{ about: nobody }} />
