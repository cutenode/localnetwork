<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import type { ProjectFormValues } from '#lib/types.ts';
	import BasicsFields from './BasicsFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Basics fields', component: BasicsFields, tags: ['autodocs'] });

	const description = 'How promoters will see you in search results.';

	const values: ProjectFormValues = {
		name: 'Neon Tide',
		pronouns: 'they/them',
		bio: 'Hypnotic, rolling techno for late rooms. Resident at Tresor, playing out since 2016.',
		city: 'Berlin',
		country: 'DE',
		links: [
			{ kind: 'soundcloud', url: 'https://soundcloud.com/neon-tide' },
			{ kind: 'instagram', url: 'https://instagram.com/neontide' }
		]
	};

	/** A picture already on the profile, drawn inline so the story never loads one. */
	const avatarUrl = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="#2a6f6f"/></svg>')}`;
</script>

<Story name="New profile" args={{ description, geocoder: false }} />

<Story name="Filled in" args={{ values, avatarUrl, description, geocoder: false }} />

<Story name="With a kind beside the name" asChild>
	<BasicsFields {values} geocoder={false} nameLabel="Project name" description="How promoters will see the project in search results." placeRequired>
		{#snippet nameAside()}
			<div>
				<label class="label" for="kind">Kind</label>
				<select id="kind" class="input" name="kind"><option value="solo">Solo</option><option value="duo">Duo</option><option value="collective">Collective</option></select>
			</div>
		{/snippet}
	</BasicsFields>
</Story>

<Story name="Phone" args={{ values, avatarUrl, description, geocoder: false }} globals={phone} />
