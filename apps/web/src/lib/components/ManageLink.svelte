<script lang="ts">
	import { Settings2 } from '@lucide/svelte';
	import type { Viewer } from '#lib/types.ts';

	/**
	 * The one way from a public page to changing it: a project's managers go to its dashboard
	 * (`section` picks a page there), staff who don't manage it to the staff console, and anyone
	 * else sees nothing.
	 */
	let {
		viewer,
		project,
		section,
		size
	}: { viewer: Viewer | null; project: { slug: string; managers: string[] }; section?: string; size?: 'sm' } = $props();

	const href = $derived(
		viewer && project.managers.includes(viewer.did)
			? `/dashboard/projects/${project.slug}${section ? `/${section}` : ''}`
			: viewer?.isStaff
				? `/admin/projects/${project.slug}`
				: null
	);
</script>

{#if href}<a {href} class={['btn-secondary', size === 'sm' && 'btn-sm']}><Settings2 class="size-4" />Manage</a>{/if}
