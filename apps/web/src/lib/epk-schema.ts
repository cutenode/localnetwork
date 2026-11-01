import { z } from 'zod';
import { readRule } from './epk.ts';

/**
 * Typed reads of the press kit records (see epk.ts). The lexicons hold the limits and run first;
 * these give the server typed values and read an access rule this app doesn't know as `none`.
 * Pages import epk.ts instead, so zod stays out of their bundles.
 */
const blob = z.object({
	$type: z.literal('blob'),
	ref: z.object({ $link: z.string() }),
	mimeType: z.string(),
	size: z.number()
});

const rule = z.string().transform(readRule);

const mix = z.object({
	title: z.string(),
	url: z.string(),
	durationMinutes: z.number().int().optional(),
	year: z.number().int().optional()
});

const photo = z.object({
	image: blob,
	alt: z.string(),
	credit: z.string().optional(),
	aspectRatio: z.object({ width: z.number().int(), height: z.number().int() }).optional()
});

export const epkRecordSchema = z.object({
	project: z.string(),
	title: z.string(),
	headline: z.string().optional(),
	bio: z.string().optional(),
	mixes: z.array(mix).optional(),
	photos: z.array(photo).optional(),
	access: z.object({ anyone: rule, promoters: rule, following: rule, bookedWith: rule }),
	createdAt: z.string(),
	updatedAt: z.string().optional()
});

export const epkContentSchema = z.object({
	bio: z.string().optional(),
	mixes: z.array(mix).optional(),
	photos: z.array(photo).optional(),
	quotes: z.array(z.object({ text: z.string(), source: z.string(), url: z.string().optional() })).optional(),
	contact: z
		.object({ email: z.string().optional(), phone: z.string().optional(), note: z.string().optional() })
		.optional(),
	techRider: z.string().optional(),
	updatedAt: z.string()
});

/** Some fields of a press kit, as a draft holds them: the public record's at the top level, the private part's under content. */
export const epkFieldsSchema = epkRecordSchema
	.pick({ title: true, headline: true, bio: true, mixes: true, photos: true, access: true })
	.partial()
	.extend({ content: epkContentSchema.omit({ updatedAt: true }).optional() });

export const epkDraftSchema = z.object({
	base: z.object({ uri: z.string(), cid: z.string() }).optional(),
	changes: epkFieldsSchema,
	previous: epkFieldsSchema.optional(),
	onBehalfOf: z.string().optional(),
	createdAt: z.string(),
	updatedAt: z.string()
});

export const epkRequestSchema = z.object({
	epk: z.string(),
	role: z.string().optional(),
	message: z.string().optional(),
	onBehalfOf: z.string().optional(),
	createdAt: z.string()
});

export const epkGrantSchema = z.object({
	subject: z.string(),
	via: z.string(),
	request: z.string().optional(),
	createdAt: z.string()
});

/** A follow record, for the one field the access rules read. */
export const followSchema = z.object({ subject: z.string() });
