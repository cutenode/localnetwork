<script lang="ts">
	import { CLAIM_RELATIONSHIPS } from '#lib/taxonomy/options.ts';
	import type { AccountView } from '#lib/types.ts';

	let { values, claimant }: { values: Record<string, string>; claimant: AccountView } = $props();

	// The proof has to name the claimant, or anyone could point a moderator at the real artist's bio.
	const proof = $derived(`localnetwork ${claimant.handle ? `@${claimant.handle}` : claimant.did}`);
</script>

<fieldset>
	<legend class="label">How are you connected?</legend>
	<div class="flex flex-col gap-2.5">
		{#each CLAIM_RELATIONSHIPS as option (option.value)}
			<label class="flex items-center gap-2.5 text-body text-fg-soft"><input type="radio" class="checkbox rounded-full" name="relationship" value={option.value} required checked={values.relationship === option.value} />{option.label}</label>
		{/each}
	</div>
</fieldset>
<div>
	<label class="label" for="evidence">How can a moderator verify this?</label>
	<textarea id="evidence" name="evidence" class="input min-h-32" required maxlength="2000" aria-describedby="evidence-hint" placeholder="For example: I added it to our SoundCloud bio.">{values.evidence ?? ''}</textarea>
	<p id="evidence-hint" class="hint">Add <code class="font-mono break-all text-fg">{proof}</code> to the project's bio, or post it from the project's account, then say where.</p>
</div>
<div>
	<label class="label" for="links">Supporting links</label>
	<textarea id="links" name="links" class="input" placeholder="One per line">{values.links ?? ''}</textarea>
</div>
