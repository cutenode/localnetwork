<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import EpkPhotos from './EpkPhotos.svelte';

	const image = (cid: string) => ({ $type: 'blob' as const, ref: { $link: cid }, mimeType: 'image/jpeg', size: 1 });

	const photos = [
		{ image: image('bafyone'), alt: 'Neon Tide at Halle Nord', credit: 'Mara Okafor' },
		{ image: image('bafytwo'), alt: 'Neon Tide behind the decks', aspectRatio: { width: 1600, height: 2000 }, onRequest: true },
		{ image: image('bafythree'), alt: 'Neon Tide outside the club' }
	];

	/**
	 * A press kit's photos, with a credit, a size, or neither under each. Storybook doesn't serve the press kit's
	 * blobs, so each photo shows its alt text in its place.
	 */
	const { Story } = defineMeta({
		title: 'Press kits/Photos',
		component: EpkPhotos,
		tags: ['autodocs'],
		args: { photos, base: '/epk/did:plc:neon/3k/photos' }
	});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Photos" />

<Story name="Phone" globals={phone} />
