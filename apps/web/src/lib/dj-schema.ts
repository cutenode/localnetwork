import { z } from 'zod';

/**
 * Schemas for what locallibrary publishes about a DJ (see dj.ts). The site doesn't publish these
 * lexicons, so it parses only the parts its pages show, leniently: an unknown field is dropped, a
 * malformed section is left out rather than failing the record. Pages import dj.ts instead, so zod
 * stays out of their bundles.
 */
const percent = z.number().int().min(0).max(100);

const bpm = z.number().int().min(20).max(400);

/** A count that's exact (`count`) or rounded down to a round number (`atLeast`). */
const count = z.union([z.object({ count: z.number().int().min(0) }), z.object({ atLeast: z.number().int().min(0) })]);

const camelot = z.string().regex(/^(1[0-2]|[1-9])[AB]$/);

const share = z.object({ name: z.string().max(200), share: percent });

const track = z.object({
	title: z.string().max(400).optional(),
	artist: z.string().max(400).optional(),
	bpmCentis: z.number().int().optional(),
	key: camelot.optional(),
	startSeconds: z.number().int().min(0).optional(),
	unidentified: z.boolean().optional()
});

/** A section that parses or is dropped, so one odd section doesn't hide the rest of a profile. */
const section = <T extends z.ZodType>(schema: T) => schema.optional().catch(undefined);

export const djProfileSchema = z.object({
	generatedAt: z.string(),
	precision: section(z.enum(['rounded', 'exact'])),
	basis: section(z.enum(['plays', 'sessions'])),
	headline: section(z.string().max(300)),
	tags: section(z.array(z.object({ name: z.string().max(100) })).max(20)),
	sets: section(
		z.object({ total: count.optional(), since: z.number().int().optional(), medianTracks: z.number().int().optional() })
	),
	tempo: section(z.object({ low: bpm, high: bpm, median: bpm })),
	arc: section(z.object({ builds: percent.optional(), steady: percent.optional(), windsDown: percent.optional() })),
	mixing: section(
		z.object({ harmonic: percent.optional(), smoothTempo: percent.optional(), genreChanges: percent.optional() })
	),
	keys: section(
		z.object({
			top: z
				.array(z.object({ key: camelot }))
				.max(12)
				.optional(),
			minor: percent.optional()
		})
	),
	energy: section(z.object({ high: percent, mid: percent, low: percent })),
	genres: section(z.array(share).max(10)),
	labels: section(z.array(share).max(10)),
	artists: section(z.array(share).max(10)),
	era: section(z.object({ medianYear: z.number().int().optional(), recent: percent.optional() })),
	signature: section(z.array(track).max(50)),
	prep: section(z.object({ hotCues: percent.optional(), memoryCues: percent.optional(), loops: percent.optional() })),
	collection: section(z.object({ tracks: count.optional(), playlists: count.optional() }))
});

export const djSetSchema = z.object({
	title: z.string().max(1000).optional(),
	date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
	kind: z.string().max(640).optional(),
	venue: z.string().max(1000).optional(),
	location: section(
		z.object({
			name: z.string().optional(),
			locality: z.string().optional(),
			region: z.string().optional(),
			country: z.string().optional()
		})
	),
	event: section(z.string().startsWith('at://')),
	playedWith: section(z.array(z.object({ name: z.string().max(640), did: z.string().optional() })).max(20)),
	links: section(z.array(z.object({ uri: z.url(), name: z.string().max(640).optional() })).max(10)),
	tracks: z.array(track).max(1000)
});
