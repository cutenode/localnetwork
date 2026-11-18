<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import StatusPill from './StatusPill.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Status pill', component: StatusPill, tags: ['autodocs'], args: { status: 'pending' } });

	const tones = {
		Good: ['approved', 'active', 'confirmed', 'success', 'verified', 'listed'],
		Warning: ['pending', 'hidden', 'negotiating', 'changes_requested', 'denied', 'outdated'],
		Critical: ['rejected', 'declined', 'failure', 'failed'],
		Info: ['open', 'written'],
		Neutral: ['paused', 'unlisted', 'self', 'unsupported', 'unverified']
	};
</script>

<Story name="Status" />

<Story name="Every status" asChild>
	<dl class="grid grid-cols-[auto_1fr] items-start gap-x-4 gap-y-3">
		{#each Object.entries(tones) as [tone, statuses] (tone)}
			<dt class="text-label text-fg-muted">{tone}</dt>
			<dd class="m-0 flex flex-wrap gap-1.5">{#each statuses as status (status)}<StatusPill {status} />{/each}</dd>
		{/each}
	</dl>
</Story>
