<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ProfileSetupCard from './ProfileSetupCard.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Profile setup card', component: ProfileSetupCard, tags: ['autodocs'] });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="From a Bluesky profile with a picture" args={{ name: 'Neon', hasAvatar: true }} />

<Story name="Without a Bluesky picture" args={{ name: 'neon.test', hasAvatar: false }} />

<Story name="Phone" args={{ name: 'Neon', hasAvatar: true }} globals={phone} />
