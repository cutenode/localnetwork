<script lang="ts">
	import { formFields } from '#lib/form-fields.svelte.ts';
	import { claimTerms } from '#lib/requests.ts';
	import type { AccountView } from '#lib/types.ts';

	/** The evidence a claim gives, on a project unless `terms` say a venue. A claim with one relationship doesn't ask for it. */
	let {
		values,
		claimant,
		terms = claimTerms({})
	}: { values: Record<string, string>; claimant: AccountView; terms?: ReturnType<typeof claimTerms> } = $props();

	const { relationships, noun, example } = $derived(terms);

	const fields = $derived(formFields(values));

	// The proof has to name the claimant, or anyone could point a moderator at the real artist's bio.
	const proof = $derived(`localnetwork ${claimant.handle ? `@${claimant.handle}` : claimant.did}`);
</script>

{#if relationships.length === 1}
	<input type="hidden" name="relationship" value={relationships[0].value} />
{:else}
	<fieldset>
		<legend class="label">How are you connected?</legend>
		<div class="flex flex-col gap-2.5">
			{#each relationships as option (option.value)}
				<label class="flex items-center gap-2.5 text-body text-fg-soft"><input type="radio" class="checkbox rounded-full" name="relationship" value={option.value} required bind:group={fields.relationship} />{option.label}</label>
			{/each}
		</div>
	</fieldset>
{/if}
<div>
	<label class="label" for="evidence">How can a moderator verify this?</label>
	<textarea id="evidence" name="evidence" class="input min-h-32" required maxlength="2000" aria-describedby="evidence-hint" placeholder={example} bind:value={fields.evidence}></textarea>
	<p id="evidence-hint" class="hint">Add <code class="font-mono break-all text-fg">{proof}</code> to the {noun}'s bio or website, or post it from the {noun}'s account, then say where.</p>
</div>
<div>
	<label class="label" for="links">Supporting links</label>
	<textarea id="links" name="links" class="input" placeholder="One per line" bind:value={fields.links}></textarea>
</div>
