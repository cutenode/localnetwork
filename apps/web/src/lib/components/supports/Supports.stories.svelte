<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ArtistSupports, Charts, SupportedArtist } from '#lib/types.ts';
	import ArtistResults from './ArtistResults.svelte';
	import ArtistSupport from './ArtistSupport.svelte';
	import ChartBoard from './ChartBoard.svelte';
	import { artistSupports, charts, supportedArtists } from './fixtures.ts';

	const { Story } = defineMeta({ title: 'Supports/Charts and artists', component: ChartBoard, tags: ['autodocs'] });

	/** A promise that never settles, for the moment before data arrives. */
	const pending = <T>() => new Promise<T>(() => {});

	const failed = <T>() => {
		const promise = Promise.reject<T>(new Error('Directory error'));
		promise.catch(() => {});

		return promise;
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const retryHref = '/charts';
</script>

<Story name="Charts" args={{ charts, retryHref }} />

<Story name="Charts, loading" args={{ charts: pending<Charts | null>(), retryHref }} />

<Story name="Charts, nothing charted" args={{ charts: { ...charts, tracks: [], artists: [] }, retryHref }} />

<Story name="Charts, offline" args={{ charts: null, retryHref }} />

<Story name="Charts, error" args={{ charts: failed<Charts | null>(), retryHref }} />

<Story name="Charts on a phone" args={{ charts, retryHref }} globals={phone} />

<Story name="Artist" asChild>
	<ArtistSupport supports={artistSupports} retryHref="/artists/ferrule" />
</Story>

<Story name="Artist, loading" asChild>
	<ArtistSupport supports={pending<ArtistSupports | null>()} retryHref="/artists/ferrule" />
</Story>

<Story name="Artist, no plays" asChild>
	<ArtistSupport supports={{ tracks: [], djs: {} }} retryHref="/artists/ferrule" />
</Story>

<Story name="Artist, offline" asChild>
	<ArtistSupport supports={null} retryHref="/artists/ferrule" />
</Story>

<Story name="Artist, error" asChild>
	<ArtistSupport supports={failed<ArtistSupports | null>()} retryHref="/artists/ferrule" />
</Story>

<Story name="Artist on a phone" asChild globals={phone}>
	<ArtistSupport supports={artistSupports} retryHref="/artists/ferrule" />
</Story>

<Story name="Artist search" asChild>
	<ArtistResults artists={supportedArtists} q="" retryHref="/artists" />
</Story>

<Story name="Artist search, loading" asChild>
	<ArtistResults artists={pending<SupportedArtist[] | null>()} q="ferr" retryHref="/artists" />
</Story>

<Story name="Artist search, no match" asChild>
	<ArtistResults artists={[]} q="zzz" retryHref="/artists" />
</Story>

<Story name="Artist search, nothing yet" asChild>
	<ArtistResults artists={[]} q="" retryHref="/artists" />
</Story>

<Story name="Artist search, offline" asChild>
	<ArtistResults artists={null} q="ferr" retryHref="/artists" />
</Story>

<Story name="Artist search on a phone" asChild globals={phone}>
	<ArtistResults artists={supportedArtists} q="" retryHref="/artists" />
</Story>
