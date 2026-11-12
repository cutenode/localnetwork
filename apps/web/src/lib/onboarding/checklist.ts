import type { Component } from 'svelte';
import type { FlagKey } from '#lib/flags/keys.generated.ts';
import type { Flags, ProfileRecord } from '#lib/types.ts';

/** What the onboarding steps check, all of it read when the dashboard home loads. */
export type OnboardingData = {
	/** The viewer's profile, or null before they make one. */
	profile: ProfileRecord | null;
	/** What a first profile starts from, from their Bluesky profile, while they have none. */
	prefill: { name: string; hasAvatar: boolean } | null;
	/** Whether staff list them in Find DJs, or they asked to be. */
	listing: boolean;
	/** Whether they follow anyone. */
	follows: boolean;
};

/** One step of the checklist, the default export of a file in steps/, which names it. */
export type StepDefinition = {
	/** Where it goes in the list, low first. Steps with the same order go by file name. */
	order: number;
	title: string;
	/** One line on what doing it gets you. */
	gets: string;
	/** Where it's done. */
	href: string;
	/** A feature's step: shown only while its flag is on for the viewer, and marked new until it's done. */
	flag?: FlagKey;
	/** Whether it's for this viewer at all, like listing in Find DJs for a DJ. Every step is, without it. */
	applies?: (data: OnboardingData) => boolean;
	done: (data: OnboardingData) => boolean;
	/** Lets the step be done right in the checklist, shown under it until it is. */
	Form?: Component<{ data: OnboardingData }>;
};

export type OnboardingStep = StepDefinition & { id: string };

/** A step as the checklist shows it. */
export type ChecklistStep = OnboardingStep & { isDone: boolean; isNew: boolean };

const fileId = (path: string) => path.slice(path.lastIndexOf('/') + 1, -'.ts'.length);

/** The steps in `files`, as `import.meta.glob` gives them, in order. */
export function collect(files: Record<string, { default: StepDefinition }>): OnboardingStep[] {
	return Object.entries(files)
		.map(([path, file]) => ({ ...file.default, id: fileId(path) }))
		.toSorted((a, b) => a.order - b.order || a.id.localeCompare(b.id));
}

/** Every onboarding step, one per file in steps/. */
export const STEPS = collect(import.meta.glob<{ default: StepDefinition }>('./steps/*.ts', { eager: true }));

/**
 * The steps `data` calls for, with whether each is done, or null once every one is. `flags` are the viewer's; without
 * them, every flag is off.
 */
export function checklist(
	steps: OnboardingStep[],
	data: OnboardingData,
	flags: Flags | undefined
): ChecklistStep[] | null {
	const shown = steps.flatMap((step) => {
		const isShown = (!step.flag || flags?.[step.flag]) && (step.applies?.(data) ?? true);
		const isDone = step.done(data);

		return isShown ? [{ ...step, isDone, isNew: !!step.flag && !isDone }] : [];
	});

	return shown.every(({ isDone }) => isDone) ? null : shown;
}
