<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import HeroHeadline from '#lib/components/HeroHeadline.svelte';
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
			eyebrow: 'For promoters',
			tone: 'aqua',
			title: 'Describe the night. Get a shortlist with reasons.',
			lede: 'The matcher scores every DJ on sound, distance, budget and room size, and tells you why each one made the list.'
		}
	});
</script>

<Story name="Start" />

<Story name="Split" args={{ level: 1, align: 'split', eyebrow: 'Shows', tone: undefined, title: 'Upcoming shows', lede: 'Where DJs on Local Network play next, near you or from the DJs you follow.' }} />

<!-- The homepage's hero header, its title the live headline. -->
<Story name="Hero" args={{ level: 1, size: 'display', align: 'center', eyebrow: undefined, lede: 'Promoters find DJs by sound, distance and budget. DJs keep a profile they own, with their booking details kept private.' }}>
	{#snippet template(args)}
		{#snippet headline()}<HeroHeadline />{/snippet}
		<SectionHeader {...args} title={headline} />
	{/snippet}
</Story>

<Story name="Eyebrow tones" asChild>
	<div class="flex flex-col gap-2">
		{#each ['low', 'mid', 'high', 'chorus', 'aqua', 'link', 'muted'] as const as tone (tone)}<Eyebrow {tone}>{tone}</Eyebrow>{/each}
	</div>
</Story>
