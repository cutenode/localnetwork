<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import About from '../About.svelte';
	import { neon, profile } from './fixtures.ts';

	/** The About tab's panel on its own; Patterns/Profile tabs draws it among the others. */
	const { Story } = defineMeta({ title: 'Profile tabs/About', component: About, tags: ['autodocs'] });

	const data = { profile: neon, sets: undefined, dj: [], playsIn: [] };

	const lookingFor = {
		...data,
		profile: profile(
			{ ...neon.record.performer, lookingFor: 'Closing slots at clubs with a long night, and a festival or two in summer.' },
			neon.record.basics.bio
		)
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Bio and highlights" args={{ data }} />

<Story name="Looking for" args={{ data: lookingFor }} />

<Story name="Only a bio" args={{ data: { ...data, profile: profile({}, neon.record.basics.bio) } }} />

<Story name="Phone" args={{ data: lookingFor }} globals={phone} />
