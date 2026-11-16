<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ProjectFormValues } from '#lib/types.ts';
	import BasicsFields from './BasicsFields.svelte';

	const { Story } = defineMeta({
		title: 'Patterns/Basics fields',
		component: BasicsFields,
		tags: ['autodocs'],
		args: { description: 'How people see you across Local Network.', geocoder: false }
	});

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

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="New profile" args={{ pronouns: '' }} />

<Story name="A project's, pronouns to edit" args={{ values }} />

<Story name="A person's, pronouns from Bluesky" args={{ values, avatarUrl, pronouns: 'she/her' }} />

<Story name="A person's, no pronouns" args={{ values: { name: 'Neon Tide' }, pronouns: '' }} />

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

<Story name="Phone, a person's" args={{ values, avatarUrl, pronouns: 'she/her' }} globals={phone} />
