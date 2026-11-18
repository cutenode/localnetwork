<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import RemoveRow from './RemoveRow.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Remove row', component: RemoveRow, tags: ['autodocs'] });
</script>

<script lang="ts">
	let links = $state(['https://soundcloud.com/neon-tide', 'https://neontide.test', 'https://instagram.com/neontide']);
</script>

<!-- Rows like the profile form's links: each one's button takes it out of the list. -->
{#snippet rows()}
	<div class="flex max-w-2xl flex-col gap-2">
		{#each links as link, index (link)}
			<div class="flex gap-2">
				<input class="input min-w-0 flex-1" name="linkUrl" value={link} aria-label="Link URL" />
				<RemoveRow label="Remove link" onclick={() => (links = links.filter((_, i) => i !== index))} />
			</div>
		{/each}
	</div>
{/snippet}

<Story name="In a row" asChild>{@render rows()}</Story>

<Story name="Phone" globals={phone} asChild>{@render rows()}</Story>
