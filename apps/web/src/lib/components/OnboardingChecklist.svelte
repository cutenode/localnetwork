<script lang="ts">
	import { Circle, CircleCheck } from '@lucide/svelte';
	import { checklist, type OnboardingData, type OnboardingStep, STEPS } from '#lib/onboarding/checklist.ts';
	import Tag from '#lib/ui/Tag.svelte';
	import type { Flags } from '#lib/types.ts';

	/**
	 * The dashboard home's checklist: one step per piece of a profile, each saying what it gets you and linking to where
	 * it's filled in. Nothing shows once every step is done.
	 */
	let { data, flags, steps = STEPS }: { data: OnboardingData; flags: Flags | undefined; steps?: OnboardingStep[] } = $props();

	const items = $derived(checklist(steps, data, flags));
</script>

{#if items}
	<section aria-labelledby="onboarding" class="card flex flex-col gap-5 p-5 sm:p-6">
		<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
			<h2 id="onboarding" class="m-0 text-emphasis font-semibold">Your checklist</h2>
			<p class="m-0 text-small text-fg-muted">{items.filter(({ isDone }) => isDone).length} of {items.length} done</p>
		</div>
		<ol class="m-0 flex list-none flex-col gap-4 p-0">
			{#each items as step (step.id)}
				<li class="flex items-start gap-3">
					{#if step.isDone}
						<CircleCheck class="mt-0.5 size-5 shrink-0 text-good" role="img" aria-label="Done" />
						<p class="m-0 text-fg-muted">{step.title}</p>
					{:else}
						<Circle class="mt-0.5 size-5 shrink-0 text-fg-subtle" aria-hidden="true" />
						<div class="flex min-w-0 flex-1 flex-col gap-0.5">
							<p class="m-0 flex flex-wrap items-center gap-2"><a class="link font-medium" href={step.href}>{step.title}</a>{#if step.isNew}<Tag variant="mono">new</Tag>{/if}</p>
							<p class="m-0 text-small text-fg-muted">{step.gets}</p>
							{#if step.Form}<div class="mt-3"><step.Form {data} /></div>{/if}
						</div>
					{/if}
				</li>
			{/each}
		</ol>
	</section>
{/if}
