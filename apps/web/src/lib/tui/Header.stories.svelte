<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The player's top row: the logo, a tab for each view, what the view is about, and localnetwork's tab at the end. */
	const { Story } = defineMeta({ title: 'Terminal/Header', component: TuiStory, tags: ['tui'] });
</script>

<Story name="Library, 80 wide" args={{ id: 'header/library-narrow' }} play={drawn('1 Library', 'DEMO', '5 localnetwork')} />

<Story name="Library, 160 wide" args={{ id: 'header/library-wide' }} play={drawn('1 Library', 'DEMO › All tracks', '5 localnetwork')} />

<Story name="Prepare, 80 wide" args={{ id: 'header/prepare-narrow' }} play={drawn('Prepare', 'on DEMO')} />

<Story name="Prepare, 160 wide" args={{ id: 'header/prepare-wide' }} play={drawn('Prepare', 'on DEMO')} />

<Story name="Drives, 80 wide" args={{ id: 'header/drives-narrow' }} play={drawn('Drives', 'DEMO', 'free')} />

<Story name="Drives, 160 wide" args={{ id: 'header/drives-wide' }} play={drawn('Drives', 'free')} />

<Story name="Recordings, 80 wide" args={{ id: 'header/recordings-narrow' }} play={drawn('Recordings', 'Sunday Warm-up')} />

<Story name="Recordings, 160 wide" args={{ id: 'header/recordings-wide' }} play={drawn('Recordings', 'Sunday Warm-up')} />

<Story name="localnetwork, 80 wide" args={{ id: 'header/localnetwork-narrow' }} play={drawn('not signed in', 'localnetwork')} />

<Story name="localnetwork, 160 wide" args={{ id: 'header/localnetwork-wide' }} play={drawn('not signed in', 'localnetwork')} />

<Story name="Player mode, 80 wide" args={{ id: 'header/player-mode-narrow' }} play={drawn('Library', 'DEMO')} />

<Story name="Player mode, 160 wide" args={{ id: 'header/player-mode-wide' }} play={drawn('Library', 'DEMO')} />

<Story name="Searching, with marks, 80 wide" args={{ id: 'header/search-and-marks-narrow' }} play={drawn('/ demo', 'marked')} />

<Story name="Searching, with marks, 160 wide" args={{ id: 'header/search-and-marks-wide' }} play={drawn('/ demo', 'marked')} />

<Story name="Searching the whole drive, 80 wide" args={{ id: 'header/search-all-narrow' }} play={drawn('/ demo', 'whole drive')} />

<Story name="Searching the whole drive, 160 wide" args={{ id: 'header/search-all-wide' }} play={drawn('/ demo', 'whole drive')} />

<Story name="Signed in, 80 wide" args={{ id: 'header/signed-in-narrow' }} play={drawn('@dj.example.com', 'localnetwork')} />

<Story name="Signed in, 160 wide" args={{ id: 'header/signed-in-wide' }} play={drawn('@dj.example.com', 'localnetwork')} />

<Story name="No drive open, 80 wide" args={{ id: 'header/no-drive-narrow' }} play={drawn('Prepare', 'no drive open')} />

<Story name="No drive open, 160 wide" args={{ id: 'header/no-drive-wide' }} play={drawn('Prepare', 'no drive open')} />
