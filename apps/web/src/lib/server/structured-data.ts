/**
 * Pages' schema.org structured data (JSON-LD), which search engines read for who or what a page is about. Each
 * builder takes the page's metadata, so the addresses in it are the same absolute ones the head names, and only the
 * fields of a project or profile it reads.
 */
import type { EpkRecord } from '#lib/epk.ts';
import type { PageMeta } from '#lib/meta.ts';
import type { MixReport } from '#lib/mix/report.ts';
import type { JsonLd, JsonLdNode } from '#lib/structured-data.ts';
import { musicBrainzUrl } from '#lib/supports.ts';
import type {
	BasicsSection,
	PerformerSection,
	ProfileRecord,
	ProfileView,
	ProjectLink,
	ProjectProfile,
	ProjectView
} from '#lib/types.ts';
import { isWebUrl } from '#lib/web-url.ts';

/** A page's structured data, from its metadata. */
export type JsonLdBuilder = (meta: PageMeta) => JsonLdNode;

/** A page's structured data in schema.org's vocabulary (see `listed` for which pages get it). */
export const jsonLdFor = (meta: PageMeta, build: JsonLdBuilder): JsonLd => ({
	'@context': 'https://schema.org',
	...build(meta)
});

/** Other pages about the same performer or person: the links they listed that a page would make clickable. */
function sameAs(links: ProjectLink[] | undefined): { sameAs?: string[] } {
	const urls = (links ?? []).map((link) => link.url).filter(isWebUrl);

	return urls.length ? { sameAs: urls } : {};
}

/** `PT1H58M12S`, the ISO 8601 duration search engines read, or undefined under a second. */
function isoDuration(seconds: number): string | undefined {
	const s = Math.round(seconds);

	if (s < 1) return undefined;
	const units = { H: Math.floor(s / 3600), M: Math.floor((s % 3600) / 60), S: s % 60 };

	return `PT${Object.entries(units)
		.map(([unit, n]) => (n ? `${n}${unit}` : ''))
		.join('')}`;
}

/** A mix report's date is whatever the DJ typed, so only a calendar date is passed on. */
const isoDate = (date: string | undefined) => (date && /^\d{4}-\d{2}-\d{2}$/.test(date) ? date : undefined);

/**
 * Someone's own picture, absolute, or nothing without one. Never the page's link preview picture, which can be a card
 * made for the page rather than a picture of them.
 */
const picture = (avatarUrl: string | null, base: string): { image?: string } =>
	avatarUrl ? { image: new URL(avatarUrl, base).href } : {};

/** What a performer's structured data reads from their project. */
type Performer = Pick<ProjectView, 'slug' | 'avatarUrl'> & {
	profile: Pick<ProjectProfile, 'name' | 'tagline' | 'genres' | 'location' | 'links'>;
};

/** A performer, as schema.org has one: a MusicGroup, which it says can be a solo musician too. */
function musicGroup(project: Performer, base: string) {
	const { name, tagline, genres, location, links } = project.profile;

	return {
		'@type': 'MusicGroup',
		name,
		url: new URL(`/projects/${project.slug}`, base).href,
		...picture(project.avatarUrl, base),
		...(tagline && { description: tagline }),
		...(genres.length > 0 && { genre: genres }),
		location: {
			'@type': 'Place',
			address: {
				'@type': 'PostalAddress',
				addressLocality: location.city,
				...(location.region && { addressRegion: location.region }),
				addressCountry: location.country
			}
		},
		...sameAs(links)
	};
}

/** A performer's page: a profile page about them. */
export const performerProfile =
	(project: Performer & Pick<ProjectView, 'createdAt' | 'updatedAt'>): JsonLdBuilder =>
	(meta) => ({
		'@type': 'ProfilePage',
		url: meta.url,
		dateCreated: project.createdAt,
		dateModified: project.updatedAt,
		mainEntity: musicGroup(project, meta.url)
	});

/** A press kit, from its public part only: a profile page about its performer, named for the kit. */
export const pressKitProfile =
	(project: Performer, record: Pick<EpkRecord, 'title' | 'headline'>): JsonLdBuilder =>
	(meta) => ({
		'@type': 'ProfilePage',
		name: record.title,
		url: meta.url,
		...(record.headline && { description: record.headline }),
		mainEntity: musicGroup(project, meta.url)
	});

/** A mix's tracklist page: the recording, by its performer. */
export const mixRecording =
	(
		project: Pick<Performer, 'slug' | 'avatarUrl'> & { profile: Pick<ProjectProfile, 'name'> },
		mix: { title: string },
		report: { mix: Pick<MixReport['mix'], 'durationSecs' | 'date'> }
	): JsonLdBuilder =>
	(meta) => {
		const duration = isoDuration(report.mix.durationSecs);
		const date = isoDate(report.mix.date);

		return {
			'@type': 'MusicRecording',
			name: mix.title,
			url: meta.url,
			byArtist: {
				'@type': 'MusicGroup',
				name: project.profile.name,
				url: new URL(`/projects/${project.slug}`, meta.url).href
			},
			...(duration && { duration }),
			...picture(project.avatarUrl, meta.url),
			...(date && { dateCreated: date })
		};
	};

/** A person's profile, when they have a profile record: a profile page about them. */
export const personProfile =
	(
		profile: Pick<ProfileView, 'avatarUrl'> & {
			record: Pick<ProfileRecord, 'createdAt' | 'updatedAt'> & {
				basics: Pick<BasicsSection, 'links'>;
				performer?: Pick<PerformerSection, 'tagline'>;
			};
		},
		name: string
	): JsonLdBuilder =>
	(meta) => ({
		'@type': 'ProfilePage',
		url: meta.url,
		dateCreated: profile.record.createdAt,
		dateModified: profile.record.updatedAt,
		mainEntity: {
			'@type': 'Person',
			name,
			url: meta.url,
			...picture(profile.avatarUrl, meta.url),
			...(profile.record.performer?.tagline && { description: profile.record.performer.tagline }),
			...sameAs(profile.record.basics.links)
		}
	});

/** An artist the directory knows by MusicBrainz ID, with MusicBrainz's page about them as the same artist. */
export const musicBrainzArtist =
	(name: string, mbid: string): JsonLdBuilder =>
	(meta) => ({
		'@type': 'MusicGroup',
		name,
		url: meta.url,
		sameAs: [musicBrainzUrl('artist', mbid)]
	});
