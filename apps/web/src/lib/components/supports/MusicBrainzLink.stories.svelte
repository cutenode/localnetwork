<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import MusicBrainzLink from './MusicBrainzLink.svelte';

	const { Story } = defineMeta({
		title: 'Supports/MusicBrainz link',
		component: MusicBrainzLink,
		tags: ['autodocs'],
		args: { kind: 'artist', mbid: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41', name: 'Ferrule' }
	});

	/** Named for what it opens, in a new tab. */
	const opensArtist = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const link = within(canvasElement).getByRole('link', { name: 'Ferrule on MusicBrainz' });

		await expect(link).toHaveAttribute('href', 'https://musicbrainz.org/artist/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');
		await expect(link).toHaveAttribute('target', '_blank');
	};
</script>

<Story name="Artist" play={opensArtist} />

<Story name="Recording" args={{ kind: 'recording', mbid: '6f1c2d84-3b9e-4c1a-9d57-0e2b8a4f7c31', name: 'Night Bus' }} />

<Story name="After a track name" asChild>
	<p class="text-small">Night Bus <MusicBrainzLink kind="recording" mbid="6f1c2d84-3b9e-4c1a-9d57-0e2b8a4f7c31" name="Night Bus" size={12} /></p>
</Story>
