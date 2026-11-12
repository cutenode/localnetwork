import type { Component } from 'svelte';
import type { DjActivity } from '#lib/dj.ts';
import type { DjSetView, ProfileView, ProjectView } from '#lib/types.ts';

/** What a profile's tabs show. The parts that stream in are undefined until they arrive, and null when the directory is offline. */
export type ProfileTabData = {
	profile: ProfileView | null;
	/** The sets the profile's mixes link to, by URI, for the mixes to show (see MixList). */
	sets: Promise<Map<string, DjSetView> | null> | undefined;
	dj: DjActivity[] | undefined;
	playsIn: ProjectView[] | null | undefined;
};

/** One tab: `id` is its `?tab=` value, tabs go in `order`, and `has` says whether this profile has anything for it. */
export type ProfileTab = {
	id: string;
	label: string;
	order: number;
	has: (data: ProfileTabData) => boolean;
};

/** A tab's file in this folder: its `tab` from `<script module>`, and the panel as the component. */
export type ProfileTabModule = { tab: ProfileTab; default: Component<{ data: ProfileTabData }> };

export type ProfileTabEntry = ProfileTab & { Panel: ProfileTabModule['default'] };
