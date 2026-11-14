<script module lang="ts">
	import type { ProfileTab, ProfileTabData } from './types.ts';

	export const tab: ProfileTab = {
		id: 'plays-in',
		label: 'Plays in',
		order: 50,
		has: ({ playsIn }) => Boolean(playsIn?.length)
	};
</script>

<script lang="ts">
	let { data }: { data: ProfileTabData } = $props();
</script>

<ul class="m-0 flex list-none flex-col gap-2 p-0">
	{#each data.playsIn ?? [] as project (project.uri)}<li><a class="link" href="/projects/{project.slug}">{project.profile.name}</a></li>{/each}
</ul>
