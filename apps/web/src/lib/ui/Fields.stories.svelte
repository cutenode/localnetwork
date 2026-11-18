<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { FEE_TIERS } from '#lib/taxonomy/options.ts';
	import Button from './Button.svelte';
	import Field, { control } from './Field.svelte';
	import TextField from './TextField.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Fields', component: TextField, tags: ['autodocs'] });
</script>

<Story name="Text" asChild>
	<div class="max-w-90"><TextField id="sound" label="Sound or name" placeholder="disco, jungle, a name" /></div>
</Story>

<Story name="With a hint" asChild>
	<div class="max-w-90"><TextField id="near" label="Near" hint="A city or postcode. Results sort by distance." placeholder="Kreuzberg, Berlin" /></div>
</Story>

<Story name="With an error" asChild>
	<div class="max-w-90"><TextField id="budget" label="Budget" value="four hundred" error="Enter an amount, like 400 or 400–800." /></div>
</Story>

<!-- Field around a control TextField doesn't draw: the control takes the ids Field hands it. -->
<Story name="Select" asChild>
	<div class="max-w-90">
		<Field id="tier" label="Fee tier" hint="Tiers are public and used for budget matching.">
			{#snippet children({ describedby, invalid })}
				<select id="tier" class={control} aria-describedby={describedby} aria-invalid={invalid || undefined}>
					{#each FEE_TIERS as tier (tier.value)}<option value={tier.value}>{tier.label}</option>{/each}
				</select>
			{/snippet}
		</Field>
	</div>
</Story>

<Story name="Text area with an error" asChild>
	<div class="max-w-90">
		<Field id="lookingFor" label="Looking for" error="Keep it under 5,000 characters.">
			{#snippet children({ describedby, invalid })}
				<textarea id="lookingFor" class={[control, 'min-h-24 py-2.5']} aria-describedby={describedby} aria-invalid={invalid || undefined}>Warm-up slots at queer parties, daytime outdoor events</textarea>
			{/snippet}
		</Field>
	</div>
</Story>

<Story name="Search form" asChild>
	<form role="search" aria-label="Find DJs" class="flex max-w-180 flex-wrap items-end gap-3" onsubmit={(event) => event.preventDefault()}>
		<div class="min-w-0 flex-[2_1_14rem]"><TextField id="form-sound" label="Sound or name" placeholder="disco, jungle, a name" /></div>
		<div class="min-w-0 flex-[1_1_10rem]"><TextField id="form-near" label="Near" placeholder="City" /></div>
		<Button type="submit">Find DJs</Button>
	</form>
</Story>
