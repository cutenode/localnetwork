<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CodeBlock from './CodeBlock.svelte';

	const { Story } = defineMeta({
		title: 'Primitives/Code block',
		component: CodeBlock,
		tags: ['autodocs'],
		argTypes: { language: { control: 'inline-radio', options: ['text', 'json', 'shell'] } },
		args: { code: 'locallibrary setup' }
	});

	const session = ['$ locallibrary check', '✓ USB-A     up to date', '! USB-B     2 playlists behind, syncing', '✓ Ready for Tidal Hours, Fri 16 Oct'].join('\n');

	const record = JSON.stringify({ title: 'Tidal Hours', date: '2026-10-16', kind: 'club', venue: 'Halle Nord', location: { locality: 'Berlin', country: 'DE' } }, null, 2);
</script>

<Story name="Command" />

<Story name="Terminal" args={{ language: 'shell', code: session }} />

<Story name="Record" args={{ language: 'json', code: record, title: 'computer.localnetwork.dj.set', meta: 'public record' }} />

<Story name="Long line scrolls" args={{ code: 'cargo install --path localnetwork/apps/locallibrary --locked && locallibrary setup --library ~/Music/rekordbox --drives /Volumes/USB-A /Volumes/USB-B' }} />
