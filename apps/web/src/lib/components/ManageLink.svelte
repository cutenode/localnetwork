<script lang="ts">
	import { Settings2 } from '@lucide/svelte';
	import type { Viewer } from '#lib/types.ts';

	/**
	 * The one way from a public page to changing it, for a project or a person's profile. A project's managers go to
	 * its dashboard (`section` picks a page there) and staff who don't manage it to the staff console; a profile's owner
	 * goes to their profile editor and staff to what that account did, in the audit log. Anyone else sees nothing.
	 */
	let {
		viewer,
		project,
		profile,
		section,
		size
	}: {
		viewer: Viewer | null;
		project?: { slug: string; managers: string[] };
		profile?: { did: string };
		section?: string;
		size?: 'sm';
	} = $props();

	const href = $derived.by(() => {
		if (!viewer) return null;

		if (project) {
			if (project.managers.includes(viewer.did)) return `/dashboard/projects/${project.slug}${section ? `/${section}` : ''}`;

			return viewer.isStaff ? `/admin/projects/${project.slug}` : null;
		}

		if (!profile) return null;

		if (profile.did === viewer.did) return '/dashboard/profile';

		return viewer.isStaff ? `/admin/audit?actor=${encodeURIComponent(profile.did)}` : null;
	});
</script>

{#if href}<a {href} class={['btn-secondary', size === 'sm' && 'btn-sm']}><Settings2 class="size-4" />Manage</a>{/if}
