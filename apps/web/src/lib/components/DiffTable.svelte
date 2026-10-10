<script lang="ts">
	import type { FieldChange } from '#lib/types.ts';

	let { changes, empty = 'No field changes' }: { changes: FieldChange[]; empty?: string } = $props();

	const pretty = (field: string) =>
		field
			.replace(/^profile\./, '')
			.replace(/\[(\d+)\]/g, ' #$1')
			.replace(/([a-z])([A-Z])/g, '$1 $2')
			.replace(/\./g, ' › ');
</script>

{#if changes.length === 0}
	<p class="muted text-sm">{empty}</p>
{:else}
	<div class="overflow-x-auto">
		<table class="table-base">
			<thead><tr><th>Field</th><th>Before</th><th>After</th></tr></thead>
			<tbody>
				{#each changes as change (change.field)}
					<tr>
						<td class="whitespace-nowrap text-ansi-white">{pretty(change.field)}</td>
						<td class="max-w-xs break-words text-ansi-bright-red">{#if change.before}<del class="decoration-ansi-red">{change.before}</del>{:else}<span class="muted">—</span>{/if}</td>
						<td class="max-w-xs break-words text-ansi-bright-green">{#if change.after}{change.after}{:else}<span class="muted">—</span>{/if}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{/if}
