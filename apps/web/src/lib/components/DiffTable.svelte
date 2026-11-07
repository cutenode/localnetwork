<script lang="ts">
	import { MoveRight } from '@lucide/svelte';
	import type { FieldChange } from '#lib/types.ts';

	let { changes, empty = 'No field changes' }: { changes: FieldChange[]; empty?: string } = $props();

	// On a phone Before and After can sit past the edge, so the table says it scrolls, and fades until it's scrolled to the end.
	let overflows = $state(false);

	let atEnd = $state(false);

	function measure(box: HTMLElement) {
		overflows = box.scrollWidth > box.clientWidth;
		atEnd = box.scrollLeft + box.clientWidth >= box.scrollWidth - 1;
	}

	const watch = (box: HTMLElement) => {
		const resizes = new ResizeObserver(() => measure(box));
		resizes.observe(box);

		// The table too: it can widen inside a box that keeps its size, as when the web fonts arrive.
		if (box.firstElementChild) resizes.observe(box.firstElementChild);

		return () => resizes.disconnect();
	};

	const pretty = (field: string) =>
		field
			.replace(/^profile\./, '')
			.replace(/\[(\d+)\]/g, ' #$1')
			.replace(/([a-z])([A-Z])/g, '$1 $2')
			.replace(/\./g, ' › ');
</script>

{#if changes.length === 0}
	<p class="text-small text-fg-muted">{empty}</p>
{:else}
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div {@attach watch} onscroll={(event) => measure(event.currentTarget)} role="region" aria-label="Changes" tabindex={overflows ? 0 : undefined} class={['overflow-x-auto', overflows && !atEnd && 'mask-r-from-[calc(100%-3rem)]']}>
		<table class="table-base">
			<thead><tr><th>Field</th><th>Before</th><th>After</th></tr></thead>
			<tbody>
				{#each changes as change (change.field)}
					<tr>
						<td class="font-mono text-label whitespace-nowrap text-fg">{pretty(change.field)}</td>
						<td class="max-w-xs break-words text-critical">{#if change.before}<del class="decoration-critical/60">{change.before}</del>{:else}<span class="text-fg-subtle">none</span>{/if}</td>
						<td class="max-w-xs break-words text-good">{#if change.after}{change.after}{:else}<span class="text-fg-subtle">none</span>{/if}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	{#if overflows}<p class="hint mb-0 flex items-center gap-1.5"><MoveRight aria-hidden="true" class="size-3.5" />Scroll sideways to see it all</p>{/if}
{/if}
