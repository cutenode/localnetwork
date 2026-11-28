<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The player's Keys screen (`change the keys`, by name in `:`): every action and its keys, the one selected told of beside the list (under it at 80 columns), what a new key would do before it's kept, and whether the keys are on the account. */
	const { Story } = defineMeta({ title: 'Terminal/Keys', component: TuiStory, tags: ['tui'] });
</script>

<Story name="As they came, 80 wide" args={{ id: 'keys/defaults-narrow' }} play={drawn('Change keys', 'enter change', 'Kept on this computer')} />

<Story name="As they came, 160 wide" args={{ id: 'keys/defaults-wide' }} play={drawn('Change keys', 'enter change', 'Kept on this computer')} />

<Story name="Filtered, 80 wide" args={{ id: 'keys/filter-narrow' }} play={drawn('13 actions', 'next track')} />

<Story name="Filtered, 160 wide" args={{ id: 'keys/filter-wide' }} play={drawn('13 actions', 'next track')} />

<Story name="Nothing matches, 80 wide" args={{ id: 'keys/no-match-narrow' }} play={drawn('No action has those words.', '0 actions')} />

<Story name="Nothing matches, 160 wide" args={{ id: 'keys/no-match-wide' }} play={drawn('No action has those words.', '0 actions')} />

<Story name="A key that can't change, 80 wide" args={{ id: 'keys/fixed-narrow' }} play={drawn("CAN'T CHANGE", 'esc close')} />

<Story name="A key that can't change, 160 wide" args={{ id: 'keys/fixed-wide' }} play={drawn("CAN'T CHANGE", 'esc close')} />

<Story name="Waiting for the key, 80 wide" args={{ id: 'keys/capture-narrow' }} play={drawn('Press the new key for next track', 'esc cancel')} />

<Story name="Waiting for the key, 160 wide" args={{ id: 'keys/capture-wide' }} play={drawn('Press the new key for next track', 'esc cancel')} />

<Story name="A taken key, 80 wide" args={{ id: 'keys/conflict-narrow' }} play={drawn('m for next track', 'enter swap')} />

<Story name="A taken key, 160 wide" args={{ id: 'keys/conflict-wide' }} play={drawn('m for next track', 'enter swap')} />

<Story name="Swapped, 80 wide" args={{ id: 'keys/swap-narrow' }} play={drawn('• m', 'next track')} />

<Story name="Swapped, 160 wide" args={{ id: 'keys/swap-wide' }} play={drawn('• m', 'next track')} />

<Story name="Back to its default, 80 wide" args={{ id: 'keys/reset-narrow' }} play={drawn('back to its default: n', 'enter reset')} />

<Story name="Back to its default, 160 wide" args={{ id: 'keys/reset-wide' }} play={drawn('back to its default: n', 'enter reset')} />

<Story name="Refused, 80 wide" args={{ id: 'keys/refused-narrow' }} play={drawn('? always opens the help', 'enter try another')} />

<Story name="Refused, 160 wide" args={{ id: 'keys/refused-wide' }} play={drawn('? always opens the help', 'enter try another')} />

<Story name="Importing, 80 wide" args={{ id: 'keys/import-preview-narrow' }} play={drawn('Keys from @dj.example.com', 'n → ]', 'left out')} />

<Story name="Importing, 160 wide" args={{ id: 'keys/import-preview-wide' }} play={drawn('Keys from @dj.example.com', 'n → ]', 'left out')} />

<Story name="Signed out, 80 wide" args={{ id: 'keys/signed-out-narrow' }} play={drawn('default n', 'Kept on this computer.')} />

<Story name="Signed out, 160 wide" args={{ id: 'keys/signed-out-wide' }} play={drawn('default n', 'Kept on this computer.')} />

<Story name="Published, 80 wide" args={{ id: 'keys/signed-in-narrow' }} play={drawn('Published to @dj.example.com')} />

<Story name="Published, 160 wide" args={{ id: 'keys/signed-in-wide' }} play={drawn('Published to @dj.example.com')} />

<Story name="Not published, 80 wide" args={{ id: 'keys/publish-error-narrow' }} play={drawn('Not published')} />

<Story name="Not published, 160 wide" args={{ id: 'keys/publish-error-wide' }} play={drawn('Not published')} />

<Story name="A keybinds.json with problems, 80 wide" args={{ id: 'keys/file-problems-narrow' }} play={drawn('keybinds.json: playback.shuffle')} />

<Story name="A keybinds.json with problems, 160 wide" args={{ id: 'keys/file-problems-wide' }} play={drawn('keybinds.json: playback.shuffle')} />

<Story name="An unreadable keybinds.json, 80 wide" args={{ id: 'keys/file-unreadable-narrow' }} play={drawn("keybinds.json can't be read")} />

<Story name="An unreadable keybinds.json, 160 wide" args={{ id: 'keys/file-unreadable-wide' }} play={drawn("keybinds.json can't be read")} />
