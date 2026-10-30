<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { waitFor } from 'storybook/test';
	import type { PlayerSource } from '#lib/mix/source.ts';
	import SetPlayer from './SetPlayer.svelte';
	import { silence, sources, tracks } from './player-fixtures.ts';

	const { Story } = defineMeta({ title: 'Mixes/Set player', component: SetPlayer, tags: ['autodocs'] });

	const recording = sources(silence(300));

	const found = (source: PlayerSource) => () => Promise.resolve(source);

	const base = { id: 'player', title: 'Tidal Hours closing set', artist: 'Neon Tide', tracks };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Plays the recording once it loads, muted, since browsers only start sound someone asked for. */
	const press = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const audio = await waitFor(() => {
			const found = canvasElement.querySelector('audio');

			if (!found) throw new Error('still loading');

			return found;
		});

		audio.muted = true;
		audio.currentTime = 100;
		// Headless browsers refuse even muted playback that no click started, which leaves it paused at 1:40.
		await audio.play().catch(() => undefined);
	};
</script>

<Story name="Loading" args={{ ...base, load: () => new Promise<PlayerSource | null>(() => {}) }} />

<Story name="A recording on the DJ's PDS" args={{ ...base, load: found(recording.pds) }} />

<Story name="Playing" args={{ ...base, load: found(recording.pds) }} play={press} />

<Story name="A plyr.fm track" args={{ ...base, load: found(recording.plyr) }} />

<Story name="An archive.org download" args={{ ...base, load: found(recording.archive) }} />

<Story name="A platform's player only" args={{ ...base, load: found(recording.embed) }} />

<Story name="Won't play" args={{ ...base, load: () => Promise.reject(new Error('player.json: HTTP 502')) }} />

<Story name="Audio that fails, with its page elsewhere" args={{ ...base, load: found({ ...recording.plyr, src: 'https://audio.invalid/missing.mp3' }) }} />

<Story name="On a phone" args={{ ...base, load: found(recording.pds) }} globals={phone} />
