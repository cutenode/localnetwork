<script lang="ts">
	import { onMount } from 'svelte';
	import type { DjActivity } from '#lib/dj.ts';
	import { PROFILE_TABS } from '#lib/profile-tabs/index.ts';
	import type { ProfileTabData } from '#lib/profile-tabs/types.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import type { ProjectView } from '#lib/types.ts';
	import EmptyState from './EmptyState.svelte';

	/**
	 * A profile's sections as tabs (a file each in lib/profile-tabs), leaving out those with nothing in them. Each tab
	 * links to `?tab=<id>`, so every tab opens without scripts too, and `requested` (that value) is the open one, or
	 * the first when the profile has no such tab. A profile with one section shows it under a heading instead. Tabs for
	 * what streams in (`dj`, `playsIn`) join once it arrives.
	 */
	let {
		requested,
		dj,
		playsIn,
		...data
	}: Pick<ProfileTabData, 'profile' | 'sets'> & {
		requested: string | null;
		dj: DjActivity[] | Promise<DjActivity[]>;
		playsIn: ProjectView[] | null | Promise<ProjectView[] | null>;
	} = $props();

	const djNow = resolved(() => dj, undefined);

	const playsInNow = resolved(() => playsIn, undefined);

	const tabData: ProfileTabData = $derived({ ...data, dj: djNow.value, playsIn: playsInNow.value });

	const tabs = $derived(PROFILE_TABS.filter((tab) => tab.has(tabData)));

	const current = $derived(tabs.find((tab) => tab.id === requested) ?? tabs[0]);

	const isPending = $derived(tabData.dj === undefined || tabData.playsIn === undefined);

	// Until scripts run, arrow keys can't move between tabs, so each stays in the tab order like any link.
	let hydrated = $state(false);

	onMount(() => {
		hydrated = true;
	});

	const moves = {
		ArrowRight: (index: number, count: number) => (index + 1) % count,
		ArrowLeft: (index: number, count: number) => (index - 1 + count) % count,
		Home: () => 0,
		End: (_: number, count: number) => count - 1
	};

	const isMove = (key: string): key is keyof typeof moves => Object.hasOwn(moves, key);

	/** The tab pattern's keys: arrows, Home and End move focus along the tabs, and Enter opens one, as for any link. */
	function onkeydown(event: KeyboardEvent & { currentTarget: HTMLAnchorElement }) {
		if (!isMove(event.key)) return;
		event.preventDefault();
		const links = [...event.currentTarget.parentElement!.querySelectorAll<HTMLAnchorElement>('[role="tab"]')];

		links[moves[event.key](links.indexOf(event.currentTarget), links.length)].focus();
	}
</script>

{#if current && tabs.length > 1}
	<div class="flex flex-col gap-6">
		<div role="tablist" aria-label="Profile" class="flex max-w-full self-start overflow-x-auto rounded-lg border border-line bg-surface p-1">
			{#each tabs as tab (tab.id)}
				{@const selected = tab === current}
				<a
					role="tab"
					id="profile-tab-{tab.id}"
					href="?tab={tab.id}"
					aria-selected={selected}
					aria-controls={selected ? 'profile-tab-panel' : undefined}
					tabindex={hydrated ? (selected ? 0 : -1) : undefined}
					data-sveltekit-replacestate
					data-sveltekit-reset="false"
					{onkeydown}
					class="flex min-h-9 shrink-0 items-center whitespace-nowrap rounded-md px-3.5 text-small text-fg-muted no-underline transition-colors hover:text-fg aria-selected:bg-well aria-selected:font-medium aria-selected:text-fg">{tab.label}</a
				>
			{/each}
		</div>
		<div role="tabpanel" id="profile-tab-panel" aria-labelledby="profile-tab-{current.id}" tabindex="0" class="min-w-0">
			<current.Panel data={tabData} />
		</div>
	</div>
{:else if current}
	<section aria-labelledby="profile-section" class="flex flex-col gap-4">
		<h2 id="profile-section" class="m-0 text-heading font-semibold">{current.label}</h2>
		<current.Panel data={tabData} />
	</section>
{:else if isPending}
	<div aria-busy="true" class="flex flex-col gap-4">
		<span class="sr-only">Loading</span>
		<div class="h-7 w-48 animate-pulse rounded-md bg-raised"></div>
		<div class="h-28 animate-pulse rounded-xl bg-surface"></div>
	</div>
{:else if data.profile}
	<EmptyState title="Nothing on this profile yet" />
{/if}
