import type { ProfileTabEntry, ProfileTabModule } from './types.ts';

/** The tabs `modules` (an eager `import.meta.glob` of this folder's tabs) define, in their order. */
export function profileTabs(modules: Record<string, ProfileTabModule>): ProfileTabEntry[] {
	return Object.values(modules)
		.map((module) => ({ ...module.tab, Panel: module.default }))
		.toSorted((a, b) => a.order - b.order);
}

/** Every tab a profile can have: a file each in this folder, so a new tab is a new file. */
export const PROFILE_TABS = profileTabs(import.meta.glob<ProfileTabModule>('./*.svelte', { eager: true }));
