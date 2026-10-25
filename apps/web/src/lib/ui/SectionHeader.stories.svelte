<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Button from './Button.svelte';
	import Eyebrow from './Eyebrow.svelte';
	import SectionHeader from './SectionHeader.svelte';

	const { Story } = defineMeta({
		title: 'Primitives/Section header',
		component: SectionHeader,
		tags: ['autodocs'],
		argTypes: {
			tone: { control: 'select', options: ['low', 'mid', 'high', 'chorus', 'aqua', 'link', 'muted'] },
			align: { control: 'inline-radio', options: ['start', 'center', 'split'] },
			size: { control: 'inline-radio', options: ['title', 'display'] }
		},
		args: {
			eyebrow: 'DJ profiles',
			tone: 'low',
			title: 'A profile built around what gets you booked.',
			lede: 'Sound, travel radius, room sizes, set lengths and fee range, so promoters can tell in one look if you fit.'
		}
	});
</script>

<Story name="Start" />

<Story name="Split" args={{ align: 'split', eyebrow: 'Mixes', tone: 'mid', title: 'Every mix, down to the transition.', lede: 'Export a set from locallibrary and the profile shows its waveform, tracklist, tempo, key moves and every blend.' }} />

<Story name="Hero" args={{ level: 1, size: 'display', align: 'center', eyebrow: undefined, title: 'The home for local DJs and the nights they play.', lede: 'Promoters find DJs by sound, distance and budget. DJs keep a profile they own, with their mixes and their nights on it.' }}>
	{#snippet template(args)}
		<SectionHeader {...args}>
			{#snippet actions()}<Button size="lg" href="/projects">Find DJs near you</Button><Button size="lg" variant="secondary" href="/dashboard/projects/new">List your project</Button>{/snippet}
		</SectionHeader>
	{/snippet}
</Story>

<Story name="Eyebrow tones" asChild>
	<div class="flex flex-col gap-2">
		{#each ['low', 'mid', 'high', 'chorus', 'aqua', 'link', 'muted'] as const as tone (tone)}<Eyebrow {tone}>{tone}</Eyebrow>{/each}
	</div>
</Story>
