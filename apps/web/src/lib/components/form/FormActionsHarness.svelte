<script lang="ts">
	import { type ComponentProps, untrack } from 'svelte';
	import FormActions from './FormActions.svelte';

	/** A small form around FormActions, for its tests and stories. `bare` draws the bar with no form around it. */
	let {
		values = { name: '' },
		rows: initialRows = 1,
		bare = false,
		form,
		submitting = false,
		kept = false,
		guardNavigation
	}: {
		values?: { name: string };
		rows?: number;
		bare?: boolean;
		form?: ComponentProps<typeof FormActions>['form'];
		submitting?: boolean;
		kept?: boolean;
		guardNavigation?: ComponentProps<typeof FormActions>['guardNavigation'];
	} = $props();

	// The count to start from: rows added after that are the harness's own.
	let rows = $state(untrack(() => initialRows));
</script>

{#snippet bar()}
	<FormActions {form} {submitting} {kept} {guardNavigation}>
		<a href="/dashboard" class="btn-ghost">Cancel</a>
		<button class="btn-primary" disabled={submitting}>{submitting ? 'Saving…' : 'Send'}</button>
	</FormActions>
{/snippet}

{#if bare}
	{@render bar()}
{:else}
	<!-- Sending is the props' job here, so the form never leaves the page. -->
	<form method="POST" class="flex flex-col gap-4" onsubmit={(event) => event.preventDefault()}>
		<div><label class="label" for="name">Name</label><input id="name" class="input" name="name" value={values.name} /></div>
		<div>
			<label class="label" for="hinted">Hinted</label>
			<input id="hinted" class="input" name="hinted" aria-describedby="hinted-hint" />
			<p id="hinted-hint" class="hint">A hint</p>
		</div>
		<div>
			<label class="label" for="genres">Genres</label>
			<div class="input flex items-center"><input id="genres" class="min-w-0 flex-1 bg-transparent" /></div>
			<input type="hidden" name="genres" value="" />
		</div>
		{#each { length: rows }, row (row)}
			<input class="input" name="row" aria-label="Row {row + 1}" />
		{/each}
		<button type="button" class="btn-ghost btn-sm self-start" onclick={() => rows++}>Add row</button>
		{@render bar()}
	</form>
{/if}
