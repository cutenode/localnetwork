<script lang="ts">
	import { untrack } from 'svelte';
	import { ACCESS_PRESETS, ACCESS_RULES, AUDIENCE_LABELS, AUDIENCES, type EpkAccess, presetFor, RULE_LABELS } from '#lib/epk.ts';

	/** Column headings short enough for three to fit beside the groups on a phone. */
	const RULE_SHORT = { auto: 'Straight in', review: 'You approve', none: "Can't ask" } as const;

	/**
	 * Who can ask to see the private part of a press kit: a ladder of presets from open to closed,
	 * or a rule per audience. Someone in several audiences gets the most open of their rules.
	 */
	let { access, preset: initial }: { access: EpkAccess; preset: string } = $props();

	// The form starts from the saved rules; the owner changes them from here.
	let preset = $state(untrack(() => initial));

	let custom = $state(untrack(() => ({ ...access })));

	const shown = $derived(ACCESS_PRESETS.find((entry) => entry.id === preset)?.access ?? custom);

	function pick(id: string) {
		preset = id;
		const chosen = ACCESS_PRESETS.find((entry) => entry.id === id);

		if (chosen) custom = { ...chosen.access };
	}

	function set(audience: (typeof AUDIENCES)[number], rule: (typeof ACCESS_RULES)[number]) {
		custom = { ...custom, [audience]: rule };
		preset = presetFor(custom)?.id ?? 'custom';
	}
</script>

<fieldset class="m-0 flex min-w-0 flex-col gap-4 border-0 p-0">
	<legend class="mb-1 text-emphasis font-semibold">Who can see the rest</legend>
	<p class="m-0 text-small text-fg-muted">Everyone sees the public part. The rest stays in this press kit's space until you let someone in.</p>
	<div class="grid gap-2 sm:grid-cols-2">
		{#each ACCESS_PRESETS as option (option.id)}
			<label class="flex cursor-pointer gap-3 rounded-xl border border-line bg-surface p-3.5 has-checked:border-fg has-focus-visible:outline-2 has-focus-visible:outline-link">
				<input type="radio" name="preset" value={option.id} checked={preset === option.id} onchange={() => pick(option.id)} class="mt-1 accent-fg" />
				<span class="flex flex-col gap-0.5">
					<span class="text-small font-semibold">{option.label}</span>
					<span class="text-label text-fg-muted">{option.description}</span>
				</span>
			</label>
		{/each}
		<label class="flex cursor-pointer gap-3 rounded-xl border border-line bg-surface p-3.5 has-checked:border-fg has-focus-visible:outline-2 has-focus-visible:outline-link">
			<input type="radio" name="preset" value="custom" checked={preset === 'custom'} onchange={() => (preset = 'custom')} class="mt-1 accent-fg" />
			<span class="flex flex-col gap-0.5">
				<span class="text-small font-semibold">Your own mix</span>
				<span class="text-label text-fg-muted">Pick a rule for each group below.</span>
			</span>
		</label>
	</div>

	<div class="overflow-x-auto rounded-xl border border-line">
		<table class="table-base w-full min-w-80">
			<caption class="sr-only">What happens when each group asks</caption>
			<thead>
				<tr>
					<th scope="col" class="text-left">When they ask</th>
					{#each ACCESS_RULES as rule (rule)}<th scope="col" class="w-18 px-1 text-center">{RULE_SHORT[rule]}</th>{/each}
				</tr>
			</thead>
			<tbody>
				{#each AUDIENCES as audience (audience)}
					<tr>
						<th scope="row" class="text-left">
							<span class="block font-sans text-small font-medium tracking-normal text-fg normal-case">{AUDIENCE_LABELS[audience].label}</span>
							<span class="block font-sans text-label tracking-normal text-fg-subtle normal-case">{AUDIENCE_LABELS[audience].hint}</span>
						</th>
						{#each ACCESS_RULES as rule (rule)}
							<td class="px-1 text-center">
								<input
									type="radio"
									name="access.{audience}"
									value={rule}
									checked={shown[audience] === rule}
									onchange={() => set(audience, rule)}
									aria-label="{AUDIENCE_LABELS[audience].label}: {RULE_LABELS[rule]}"
									class="accent-fg"
								/>
							</td>
						{/each}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="m-0 text-label text-fg-subtle">Someone in more than one group gets the most open rule. You can always add people yourself.</p>
</fieldset>
