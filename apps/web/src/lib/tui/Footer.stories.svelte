<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The player's two bottom rows: what's under way and where things are, then what the keys do, always ending with the commands and help. */
	const { Story } = defineMeta({ title: 'Terminal/Footer', component: TuiStory, tags: ['tui'] });
</script>

<Story name="Library, 80 wide" args={{ id: 'footer/library-narrow' }} play={drawn('enter', '? help')} />

<Story name="Library, 160 wide" args={{ id: 'footer/library-wide' }} play={drawn('enter', 'e actions', '? help')} />

<Story name="Prepare, 80 wide" args={{ id: 'footer/prepare-narrow' }} play={drawn('enter run', '? help')} />

<Story name="Prepare, 160 wide" args={{ id: 'footer/prepare-wide' }} play={drawn('enter run', '? help')} />

<Story name="Drives, 80 wide" args={{ id: 'footer/drives-narrow' }} play={drawn('enter browse', '? help')} />

<Story name="Drives, 160 wide" args={{ id: 'footer/drives-wide' }} play={drawn('enter browse', '? help')} />

<Story name="Recordings, 80 wide" args={{ id: 'footer/recordings-narrow' }} play={drawn('e actions', '? help')} />

<Story name="Recordings, 160 wide" args={{ id: 'footer/recordings-wide' }} play={drawn('e actions', '? help')} />

<Story name="localnetwork, 80 wide" args={{ id: 'footer/localnetwork-narrow' }} play={drawn('e actions', '? help')} />

<Story name="localnetwork, 160 wide" args={{ id: 'footer/localnetwork-wide' }} play={drawn('e actions', '? help')} />

<Story name="Player mode, 80 wide" args={{ id: 'footer/player-mode-narrow' }} play={drawn('hot cue', '? help')} />

<Story name="Player mode, 160 wide" args={{ id: 'footer/player-mode-wide' }} play={drawn('hot cue', '? help')} />

<Story name="Searching, 80 wide" args={{ id: 'footer/search-narrow' }} play={drawn('SEARCH', '/ demo')} />

<Story name="Searching, 160 wide" args={{ id: 'footer/search-wide' }} play={drawn('SEARCH', '/ demo')} />

<Story name="Searching the whole drive, 80 wide" args={{ id: 'footer/search-all-narrow' }} play={drawn('SEARCH', 'in the whole drive')} />

<Story name="Searching the whole drive, 160 wide" args={{ id: 'footer/search-all-wide' }} play={drawn('SEARCH', 'in the whole drive')} />

<Story name="A search that can't be read, 80 wide" args={{ id: 'footer/search-error-narrow' }} play={drawn('SEARCH', 'bpm: needs a number')} />

<Story name="A search that can't be read, 160 wide" args={{ id: 'footer/search-error-wide' }} play={drawn('SEARCH', 'bpm: needs a number')} />

<Story name="Typing a name, 80 wide" args={{ id: 'footer/prompt-narrow' }} play={drawn('TYPING', 'Warm-up')} />

<Story name="Typing a name, 160 wide" args={{ id: 'footer/prompt-wide' }} play={drawn('TYPING', 'Warm-up')} />

<Story name="Picking a playlist, 80 wide" args={{ id: 'footer/picking-narrow' }} play={drawn('PICK', 'to which playlist')} />

<Story name="Picking a playlist, 160 wide" args={{ id: 'footer/picking-wide' }} play={drawn('PICK', 'to which playlist')} />

<Story name="Pads on, 80 wide" args={{ id: 'footer/pads-narrow' }} play={drawn('PADS')} />

<Story name="Pads on, 160 wide" args={{ id: 'footer/pads-wide' }} play={drawn('PADS')} />

<Story name="After one esc, 80 wide" args={{ id: 'footer/quit-narrow' }} play={drawn('esc again to quit')} />

<Story name="After one esc, 160 wide" args={{ id: 'footer/quit-wide' }} play={drawn('esc again to quit')} />

<Story name="Compact hints, 80 wide" args={{ id: 'footer/hints-compact-narrow' }} play={drawn('? help')} />

<Story name="Compact hints, 160 wide" args={{ id: 'footer/hints-compact-wide' }} play={drawn('? help')} />

<Story name="Writing the drive, 80 wide" args={{ id: 'footer/job-narrow' }} play={drawn('Writing TEST', 'esc cancel', '& jobs')} />

<Story name="Writing the drive, 160 wide" args={{ id: 'footer/job-wide' }} play={drawn('Writing TEST', 'esc cancel', '& jobs')} />

<Story name="Too far in to stop, 80 wide" args={{ id: 'footer/job-committed-narrow' }} play={drawn('Writing TEST', "can't stop now", '& jobs')} />

<Story name="Too far in to stop, 160 wide" args={{ id: 'footer/job-committed-wide' }} play={drawn('Writing TEST', "can't stop now", '& jobs')} />

<Story name="First run, 80 wide" args={{ id: 'footer/intro-narrow' }} play={drawn('new here?', '? help')} />

<Story name="First run, 160 wide" args={{ id: 'footer/intro-wide' }} play={drawn('new here?', '? help')} />

<Story name="No drive open, 80 wide" args={{ id: 'footer/no-drive-narrow' }} play={drawn('enter run', '? help')} />

<Story name="No drive open, 160 wide" args={{ id: 'footer/no-drive-wide' }} play={drawn('enter run', '? help')} />
