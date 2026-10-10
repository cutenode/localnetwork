/** Labelled option lists for the closed-ish vocabularies in the lexicons. */
export type Option<T extends string = string> = { value: T; label: string; hint?: string };

export const PROJECT_KINDS = [
	{ value: 'solo', label: 'Solo DJ' },
	{ value: 'duo', label: 'Duo / B2B pair' },
	{ value: 'group', label: 'Group' },
	{ value: 'collective', label: 'Collective / crew' },
	{ value: 'live', label: 'Live act' }
] as const satisfies readonly Option[];

export const FORMATS = [
	{ value: 'vinyl', label: 'Vinyl' },
	{ value: 'cdj', label: 'CDJs / USB' },
	{ value: 'controller', label: 'Controller' },
	{ value: 'hybrid', label: 'Hybrid' },
	{ value: 'live', label: 'Live hardware' },
	{ value: 'modular', label: 'Modular' }
] as const satisfies readonly Option[];

export const SET_TYPES = [
	{ value: 'opener', label: 'Opener' },
	{ value: 'warmup', label: 'Warm-up' },
	{ value: 'peak', label: 'Peak time' },
	{ value: 'closing', label: 'Closing' },
	{ value: 'allNight', label: 'All night long' },
	{ value: 'b2b', label: 'B2B' },
	{ value: 'daytime', label: 'Daytime / day party' },
	{ value: 'afterhours', label: 'Afterhours' },
	{ value: 'liveSet', label: 'Live set' }
] as const satisfies readonly Option[];

export const FEE_TIERS = [
	{ value: 'community', label: 'Community', hint: 'DIY, benefit, and community shows' },
	{ value: 'emerging', label: 'Emerging', hint: 'Club nights and local support slots' },
	{ value: 'established', label: 'Established', hint: 'Regular headliners in their scene' },
	{ value: 'headliner', label: 'Headliner', hint: 'Touring and festival headliners' }
] as const satisfies readonly Option[];

export const LINK_KINDS = [
	{ value: 'website', label: 'Website' },
	{ value: 'soundcloud', label: 'SoundCloud' },
	{ value: 'mixcloud', label: 'Mixcloud' },
	{ value: 'bandcamp', label: 'Bandcamp' },
	{ value: 'instagram', label: 'Instagram' },
	{ value: 'bluesky', label: 'Bluesky' },
	{ value: 'residentAdvisor', label: 'Resident Advisor' },
	{ value: 'youtube', label: 'YouTube' },
	{ value: 'spotify', label: 'Spotify' },
	{ value: 'twitch', label: 'Twitch' },
	{ value: 'other', label: 'Other' }
] as const satisfies readonly Option[];

export const CLAIM_RELATIONSHIPS = [
	{ value: 'self', label: 'This is me / my project' },
	{ value: 'member', label: "I'm a member of this group" },
	{ value: 'manager', label: "I'm their manager" },
	{ value: 'agent', label: "I'm their booking agent" }
] as const satisfies readonly Option[];

export const BOOKING_STATUSES = [
	{ value: 'open', label: 'Open' },
	{ value: 'negotiating', label: 'Negotiating' },
	{ value: 'confirmed', label: 'Confirmed' },
	{ value: 'declined', label: 'Declined' },
	{ value: 'cancelled', label: 'Cancelled' }
] as const satisfies readonly Option[];

export type FeeTier = (typeof FEE_TIERS)[number]['value'];

export const FEE_TIER_ORDER: FeeTier[] = FEE_TIERS.map((tier) => tier.value);

export function labelFor(options: readonly Option[], value: string | null | undefined): string {
	if (!value) return '';

	return options.find((option) => option.value === value)?.label ?? value;
}

export function isOneOf<T extends string>(options: readonly Option<T>[], value: unknown): value is T {
	return options.some((option) => option.value === value);
}
