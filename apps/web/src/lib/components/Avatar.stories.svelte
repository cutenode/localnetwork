<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Avatar from './Avatar.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Avatar', component: Avatar, tags: ['autodocs'], args: { name: 'Neon Tide' } });

	// A photo drawn inline, so the story never waits on the network.
	const photo = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x2="1" y2="1"><stop offset="0" stop-color="#1e3a8a"/><stop offset="1" stop-color="#2dd4bf"/></linearGradient></defs><rect width="64" height="64" fill="url(#g)"/><circle cx="32" cy="26" r="11" fill="#e0f2fe"/><path d="M12 64a20 18 0 0 1 40 0z" fill="#e0f2fe"/></svg>')}`;

	const sizes = ['sm', 'md', 'lg', 'xl'] as const;
</script>

<Story name="Photo" args={{ src: photo }} />

<Story name="Monogram fallback" />

<Story name="Sizes" asChild>
	<div class="flex flex-col gap-3">
		<div class="flex items-end gap-3">{#each sizes as size (size)}<Avatar src={photo} name="Neon Tide" {size} />{/each}</div>
		<div class="flex items-end gap-3">{#each sizes as size (size)}<Avatar name="Neon Tide" {size} />{/each}</div>
	</div>
</Story>
