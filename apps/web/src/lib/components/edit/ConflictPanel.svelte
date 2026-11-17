<script lang="ts">
	import type { ConflictField } from './types.ts';

	/**
	 * A save that found the page changed since editing began (the stale-write check). Fields only the other person
	 * changed are already taken in; each field both changed is a choice between theirs and yours, yours picked to start
	 * with. Saving again keeps the picks.
	 */
	let { fields = $bindable() }: { fields: ConflictField[] } = $props();
</script>

<div class="flex flex-col gap-4 px-4 py-4">
	<p class="m-0 text-small text-fg-soft">
		Someone saved this page after you started. Their other changes are already in.{fields.length ? ' Pick what to keep where you both changed something.' : ''}
	</p>
	{#each fields as field, index (field.name)}
		<fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
			<legend class="mb-2 p-0 text-label font-medium text-fg-soft">{field.label}</legend>
			<div class="grid gap-2 sm:grid-cols-2">
				{#each [{ value: 'theirs', title: 'Theirs', text: field.theirs }, { value: 'yours', title: 'Yours', text: field.yours }] as const as option (option.value)}
					<label class={['flex cursor-pointer flex-col gap-1 rounded-lg border p-3 transition-colors', field.keep === option.value ? 'border-fg bg-well' : 'border-line-strong hover:border-fg-subtle']}>
						<span class="flex items-center gap-2 text-label text-fg-muted">
							<!-- Its own form (one that doesn't exist), so the picks aren't sent with the page's fields: they're written into them. -->
							<input
								type="radio"
								form="edit-conflict"
								class="checkbox rounded-full"
								name="keep-{field.name}"
								value={option.value}
								checked={field.keep === option.value}
								onchange={() => (fields = fields.map((each, at) => (at === index ? { ...each, keep: option.value } : each)))}
							/>
							{option.title}
						</span>
						<span class="line-clamp-2 text-small text-fg">{option.text}</span>
					</label>
				{/each}
			</div>
		</fieldset>
	{/each}
</div>
