<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import BasicsFields from './BasicsFields.svelte';

	const { Story } = defineMeta({
		title: 'Patterns/Basics fields',
		component: BasicsFields,
		tags: ['autodocs'],
		args: { description: 'How people see you across Local Network.', geocoder: false }
	});

	const values = { name: 'Neon Tide', pronouns: 'they/them', bio: 'Rolling techno from Leipzig.' };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="A project's, pronouns to edit" args={{ values }} />

<Story name="A person's, pronouns from Bluesky" args={{ values, pronouns: 'she/her' }} />

<Story name="A person's, no pronouns" args={{ values: { name: 'Neon Tide' }, pronouns: '' }} />

<Story name="Phone, a person's" args={{ values, pronouns: 'she/her' }} globals={phone} />
