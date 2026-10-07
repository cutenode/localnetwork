<script lang="ts">
	import { CLAIM_RELATIONSHIPS } from '#lib/taxonomy/options.ts';

	let { values }: { values: Record<string, string> } = $props();
</script>

<fieldset>
	<legend class="label">How are you connected?</legend>
	<div class="space-y-2">
		{#each CLAIM_RELATIONSHIPS as option (option.value)}
			<label class="flex items-center gap-2 text-sm"><input type="radio" class="checkbox rounded-full" name="relationship" value={option.value} required checked={values.relationship === option.value} />{option.label}</label>
		{/each}
	</div>
</fieldset>
<div>
	<label class="label" for="evidence">How can a moderator verify this?</label>
	<textarea id="evidence" name="evidence" class="input min-h-32" required maxlength="2000" placeholder="For example: I've put 'localnetwork' in my SoundCloud bio, or DM the project's Instagram from our account.">{values.evidence ?? ''}</textarea>
</div>
<div>
	<label class="label" for="links">Supporting links</label>
	<textarea id="links" name="links" class="input" placeholder="One per line">{values.links ?? ''}</textarea>
</div>
