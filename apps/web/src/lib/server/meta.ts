import { type EpkRecord, type EpkView, pressKitPath } from '#lib/epk.ts';
import { kindLabel, placeLabel, pluralize, profileHref } from '#lib/format.ts';
import { type PageImage, type PageMeta, SITE_NAME } from '#lib/meta.ts';
import { CARD } from '#lib/og/style.ts';
import { bareMbid } from '#lib/supports.ts';
import type { AccountView, ProfileRecord, ProfileView, ProjectView } from '#lib/types.ts';
import { isDrawable } from './og/drawable';
import { cardPaths } from './og/paths';

/** About as much of a description as link previews show; longer ones are cut at a word. */
const DESCRIPTION_LENGTH = 200;

type MetaInput = Omit<PageMeta, 'url' | 'type' | 'image'> & {
	/** The page's canonical path. */
	path: string;
	type?: PageMeta['type'];
	/** A picture on this site, by its path. The site's card when left out. */
	image?: Omit<PageImage, 'url'> & { path: string };
};

/** One of the cards the site draws for link previews (routes/og). */
const card = (path: string, alt: string) => ({ path, alt, ...CARD });

const SITE_CARD = card(cardPaths.site, `${SITE_NAME}: find local DJs for your show`);

/**
 * One of a performer's cards, while they're active. Otherwise the site's: crawlers ask signed out, and a card follows
 * its page, which only managers and staff can open then. A name the card's fonts can't draw gets the site's too.
 */
const performerCard = (project: ProjectView, path: string, alt: string) =>
	project.status === 'active' && isDrawable(project.profile.name) ? card(path, alt) : undefined;

/** A page's metadata, its addresses made absolute against the site's public URL so every page names the same host. */
export function pageMeta(
	publicUrl: string,
	{ path, type = 'website', image = SITE_CARD, description, ...rest }: MetaInput
): PageMeta {
	const { path: imagePath, ...picture } = image;

	return {
		...rest,
		type,
		description: clip(description),
		url: new URL(path, publicUrl).href,
		image: { url: new URL(imagePath, publicUrl).href, ...picture }
	};
}

function clip(text: string): string {
	const flat = text.replace(/\s+/g, ' ').trim();

	if (flat.length <= DESCRIPTION_LENGTH) return flat;
	const cut = flat.slice(0, DESCRIPTION_LENGTH - 1);
	const space = cut.lastIndexOf(' ');

	return `${(space > 0 ? cut.slice(0, space) : cut).replace(/[\s,.;:]+$/, '')}…`;
}

/** A performer's one-line summary: their tagline, or what and where they are. */
const summary = (profile: ProjectView['profile']) =>
	profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}`;

/** What a performer's card says of them: "Neon Tide, a solo DJ from Berlin". */
function performerAlt({ name, kind, location }: ProjectView['profile']): string {
	const label = kindLabel(kind).split(' / ')[0];
	const noun = label.charAt(0).toLowerCase() + label.slice(1);

	return `${name}, ${/^[aeiou]/i.test(noun) ? 'an' : 'a'} ${noun} from ${location.city}`;
}

/**
 * What every page of a performer's shares: it's about them, it shows their card, and while they aren't active only
 * their managers and staff can open it, so search engines are told to skip it.
 */
const performerPage = (project: ProjectView) => ({
	type: 'profile' as const,
	image: performerCard(project, cardPaths.project(project), performerAlt(project.profile)),
	noindex: project.status !== 'active'
});

/** A performer's page. Pages of performers who aren't active only open for their managers and staff. */
export const performerMeta = (publicUrl: string, project: ProjectView): PageMeta =>
	pageMeta(publicUrl, {
		title: project.profile.name,
		description: summary(project.profile),
		path: `/projects/${project.slug}`,
		...performerPage(project)
	});

/** A performer's press kits, listed. */
export const pressKitsMeta = (publicUrl: string, project: ProjectView): PageMeta =>
	pageMeta(publicUrl, {
		title: `${project.profile.name} press kit`,
		description: `Press kit for ${project.profile.name}: ${summary(project.profile)}`,
		path: `/projects/${project.slug}/press-kits`,
		...performerPage(project)
	});

/** What a press kit's metadata reads, all of it from the kit's public part. */
type PressKitSummary = Pick<EpkView, 'rkey' | 'cid'> & {
	record: Pick<EpkRecord, 'title' | 'headline'> & { photos?: { image: { ref: { $link: string } }; alt: string }[] };
};

/** One press kit. Its card shows the first press photo, which is in the public part, or it's the performer's card. */
export function pressKitMeta(
	publicUrl: string,
	project: ProjectView,
	{ rkey, cid, record }: PressKitSummary
): PageMeta {
	const path = pressKitPath(project.slug, rkey);
	const photo = record.photos?.[0];
	const page = performerPage(project);

	return pageMeta(publicUrl, {
		title: `${project.profile.name}: ${record.title}`,
		description: `${project.profile.name}'s press kit: ${record.headline ?? summary(project.profile)}`,
		path,
		...page,
		image: photo
			? performerCard(
					project,
					cardPaths.pressKit(project.slug, { rkey, cid }),
					`${[project.profile.name, record.headline].filter(Boolean).join(', ')}, over their press photo${photo.alt ? `: ${photo.alt}` : ''}`
				)
			: page.image
	});
}

/** What a profile's metadata reads from it. */
type ProfileSummary = Pick<ProfileView, 'avatarUrl'> & {
	record: {
		basics: Pick<ProfileRecord['basics'], 'location'>;
		performer?: Pick<NonNullable<ProfileRecord['performer']>, 'tagline'>;
	};
};

/** A person's profile, at their current handle. An account without a profile record has only its name. */
export function profileMeta(
	publicUrl: string,
	account: AccountView,
	profile: ProfileSummary | null,
	name: string
): PageMeta {
	const location = profile?.record.basics.location;
	const place = location && placeLabel({ location });

	return pageMeta(publicUrl, {
		title: name,
		description: profile?.record.performer?.tagline ?? (place ? `${name}, ${place}` : name),
		path: profileHref(account),
		type: 'profile',
		image: profile?.avatarUrl ? { path: profile.avatarUrl, alt: `${name}'s picture` } : undefined
	});
}

/**
 * An artist's page: which DJs played their tracks. One no set credits yet, named only by what was typed into its
 * address, is kept out of search.
 */
export function artistMeta(publicUrl: string, name: string | null, path: string, credited: boolean): PageMeta {
	// Only an artist with a MusicBrainz ID, whose page is /artists/<mbid>, has a card.
	const mbid = path.startsWith('/artists/') ? bareMbid(path.slice('/artists/'.length)) : undefined;

	return pageMeta(publicUrl, {
		title: name ?? 'Artist',
		description: `Which Local Network DJs played ${name ? `${name}'s` : "this artist's"} tracks, and where.`,
		path,
		image:
			mbid && credited && name && isDrawable(name)
				? card(cardPaths.artist(mbid), `${name}: played by Local Network DJs, with their most played tracks`)
				: undefined,
		noindex: !credited
	});
}

/** A mix's tracklist page, the nth mix on its performer's page. */
export const mixMeta = (
	publicUrl: string,
	project: ProjectView,
	mix: { title: string },
	n: number,
	tracks: number
): PageMeta =>
	pageMeta(publicUrl, {
		title: `${mix.title} by ${project.profile.name}`,
		description: `The tracklist of ${mix.title} by ${project.profile.name}: ${pluralize(tracks, 'track')}, when each plays, and how they were mixed.`,
		path: `/projects/${project.slug}/mixes/${n}`,
		...performerPage(project),
		image: performerCard(
			project,
			cardPaths.mix(project, n),
			`${mix.title} by ${project.profile.name}, ${pluralize(tracks, 'track')}, with the mix's waveform`
		),
		type: 'music.song'
	});
