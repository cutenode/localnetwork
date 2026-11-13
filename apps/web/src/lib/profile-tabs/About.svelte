<script module lang="ts">
	import type { ProfileTab, ProfileTabData } from './types.ts';

	export const tab: ProfileTab = {
		id: 'about',
		label: 'About',
		order: 10,
		has: ({ profile }) =>
			Boolean(
				profile?.record.basics.bio || profile?.record.performer?.lookingFor || profile?.record.performer?.highlights?.length
			)
	};
</script>

<script lang="ts">
	let { data }: { data: ProfileTabData } = $props();

	const basics = $derived(data.profile?.record.basics);

	const performer = $derived(data.profile?.record.performer);
</script>

<div class="flex flex-col gap-5">
	{#if basics?.bio}<p class="m-0 max-w-[65ch] whitespace-pre-line text-fg-soft">{basics.bio}</p>{/if}
	{#if performer?.lookingFor}
		<div class="flex flex-col gap-1">
			<h3 class="m-0 eyebrow text-fg-subtle">Looking for</h3>
			<p class="m-0 max-w-[65ch] whitespace-pre-line text-small text-fg-muted">{performer.lookingFor}</p>
		</div>
	{/if}
	{#if performer?.highlights?.length}
		<div class="flex flex-col gap-2">
			<h3 class="m-0 eyebrow text-fg-subtle">Highlights</h3>
			<ul class="m-0 flex list-none flex-col gap-2 p-0">
				{#each performer.highlights as highlight, index (index)}<li class="border-l-2 border-line-strong pl-3 text-fg-soft">{highlight}</li>{/each}
			</ul>
		</div>
	{/if}
</div>
