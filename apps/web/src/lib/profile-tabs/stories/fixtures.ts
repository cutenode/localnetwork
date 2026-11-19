import { NEON_CREW, neonPlaying, neonProfile } from '#lib/testing/profiles.ts';
import type { ProfileView } from '#lib/types.ts';

/** Neon's profile tabs for the stories, with Neon from the seed data. */

/** Neon's profile with `performer`, and `bio` when there's one. */
export const profile = (performer: NonNullable<ProfileView['record']['performer']>, bio?: string) =>
	neonProfile({ basics: { name: 'Neon', bio }, performer });

export const neon = profile(
	{
		mixes: [{ title: 'Warehouse Hours', url: 'https://soundcloud.com/neon/warehouse' }],
		highlights: ['Four-year residency at Halle Nord']
	},
	'Plays long techno sets in Berlin, and runs a monthly warehouse night with friends.'
);

export const dj = neonPlaying({ stats: true });

export const playsIn = [NEON_CREW];

/** What streams in, never arriving. */
export const waiting = new Promise<never>(() => {});
