import { error } from '@sveltejs/kit';
import {
	type EpkConflict,
	type EpkConflictView,
	type EpkDraftView,
	type EpkField,
	type EpkFields,
	type EpkFieldValue,
	type EpkPhoto,
	type EpkView,
	presetFor,
	pressKitPath
} from '#lib/epk.ts';
import type { AccountView, ProjectView, Viewer } from '#lib/types.ts';
import { parseEpkForm } from './forms/epk';
import { requireManager } from './manage';
import type { AccountService } from './services/accounts';
import type { EpkService } from './services/epk';
import { applyChanges, EPK_FIELDS, fieldsFrom, fieldsOf, photosOf } from './services/epk-fields';
import type { ProjectService } from './services/projects';

/** What the press kit dashboard pages read from `locals`. */
type Locals = {
	viewer: Viewer | null;
	app: { projects: Pick<ProjectService, 'bySlug'>; epks: EpkService; accounts: Pick<AccountService, 'view'> };
};

/** Where an old `/epk/<did>/<rkey>` link (and `suffix` under it) lives now, or a 404. */
export async function movedPath(epks: EpkService, did: string, rkey: string, suffix = ''): Promise<string> {
	const found = await epks.locate(did, rkey);

	if (!found) error(404, 'Press kit not found');

	return `${pressKitPath(found.project.slug, found.epk.rkey)}${suffix}`;
}

/**
 * A press kit of project `slug` for its dashboard pages, which any of the project's managers can change. One still in
 * its maker's repo moves to the root first when its maker opens it; for the others, `legacyMaker` says who has to.
 */
export async function managedKit(locals: Locals, url: URL, params: { slug: string; rkey: string }) {
	const { viewer, project } = await requireManager(locals, url, params.slug);
	let kit = await locals.app.epks.get(project, params.rkey);

	if (kit?.epk.legacy && kit.epk.did === viewer.did) {
		await locals.app.epks.migrateLegacy(viewer);
		kit = await locals.app.epks.get(project, params.rkey);
	}

	if (!kit) error(404, 'Press kit not found');

	return { viewer, kit, legacyMaker: kit.epk.legacy ? await locals.app.accounts.view(kit.epk.did) : null };
}

/**
 * What the editor shows for the press kit `rkey`: the viewer's draft if they have one, laid over the published press
 * kit, and the other managers' drafts of it. A draft of a press kit that isn't published yet opens only for its author.
 */
export async function loadEditor(
	locals: Locals,
	url: URL,
	params: { slug: string; rkey: string }
): Promise<{
	viewer: Viewer;
	project: ProjectView;
	epk: EpkView | null;
	draft: EpkDraftView | null;
	others: EpkDraftView[];
	published: EpkFields;
	fields: EpkFields;
	legacyMaker: AccountView | null;
}> {
	const { viewer, project } = await requireManager(locals, url, params.slug);
	const { epks } = locals.app;
	let entry = await findEntry(epks, project, params.rkey);

	if (entry.epk?.legacy && entry.epk.did === viewer.did) {
		await epks.migrateLegacy(viewer);
		entry = await findEntry(epks, project, params.rkey);
	}

	const { epk, drafts } = entry;
	const draft = drafts.find((item) => item.author.did === viewer.did) ?? null;

	if (!epk && !draft) error(404, 'Press kit not found');
	const legacyMaker = epk?.legacy ? await locals.app.accounts.view(epk.did) : null;
	const published = epk && !epk.legacy ? fieldsOf(epk.record, await epks.content(epk)) : {};

	return {
		viewer,
		project,
		epk,
		draft,
		others: drafts.filter((item) => item.author.did !== viewer.did),
		published,
		fields: draft ? applyChanges(published, draft.changes) : published,
		legacyMaker
	};
}

async function findEntry(epks: EpkService, project: ProjectView, rkey: string) {
	const { kits, drafts } = await epks.dashboard(project);
	const kit = kits.find((entry) => entry.epk.rkey === rkey);

	return { epk: kit?.epk ?? null, drafts: kit?.drafts ?? drafts.filter((draft) => draft.rkey === rkey) };
}

/** The photos the form can keep, looked up by CID: the published press kit's and the viewer's draft's. */
export const editorPhotos = (editor: { published: EpkFields; draft: EpkDraftView | null }): EpkPhoto[] => [
	...photosOf(editor.published),
	...photosOf(editor.draft?.changes ?? {})
];

/** Saves the press kit form as the viewer's draft: new photos go to the repo the draft is written to first. */
export async function saveDraftForm(
	epks: EpkService,
	viewer: Viewer,
	project: ProjectView,
	form: FormData,
	existingPhotos: EpkPhoto[],
	rkey?: string
): Promise<EpkDraftView | null> {
	const parsed = await parseEpkForm(form, project.profile.mixes ?? [], existingPhotos);
	const publicPhotos = [...parsed.publicPhotos];
	const privatePhotos = [...parsed.privatePhotos];

	for (const upload of parsed.uploads) {
		const photo: EpkPhoto = {
			image: await epks.drafts.uploadPhoto(viewer, upload.bytes, upload.mimeType),
			alt: `${project.profile.name}, press photo`
		};

		if (upload.credit) photo.credit = upload.credit;

		if (upload.aspectRatio) photo.aspectRatio = upload.aspectRatio;
		(upload.visibility === 'public' ? publicPhotos : privatePhotos).push(photo);
	}

	const values = {
		title: parsed.title,
		headline: parsed.headline,
		bio: parsed.bio,
		access: parsed.access,
		mixes: parsed.publicMixes,
		photos: publicPhotos,
		content: { ...parsed.content, mixes: parsed.privateMixes, photos: privatePhotos }
	};

	return epks.drafts.save(viewer, project, { rkey, cid: parsed.cid, values });
}

const FIELD_LABELS: Record<EpkField, string> = {
	title: 'Name',
	headline: 'Headline',
	bio: 'Short bio',
	mixes: 'Mixes for everyone',
	photos: 'Photos for everyone',
	access: 'Who can ask',
	'content.bio': 'Full bio',
	'content.mixes': 'Mixes on request',
	'content.photos': 'Photos on request',
	'content.quotes': 'Press quotes',
	'content.contact': 'Contact',
	'content.techRider': 'Tech rider'
};

const count = (items: { length: number } | undefined, noun: string) =>
	items?.length ? `${items.length} ${noun}${items.length === 1 ? '' : 's'}` : '';

/** A field's value in words, for comparing versions side by side. */
function describe(field: EpkField, value: EpkFieldValue | undefined): string {
	const fields = fieldsFrom([[field, value]]);
	const { content } = fields;

	const words: Record<EpkField, string | undefined> = {
		title: fields.title,
		headline: fields.headline,
		bio: fields.bio,
		mixes: fields.mixes?.map((mix) => mix.title).join(', '),
		photos: count(fields.photos, 'photo'),
		access: fields.access && (presetFor(fields.access)?.label ?? 'Your own mix'),
		'content.bio': content?.bio,
		'content.mixes': content?.mixes?.map((mix) => mix.title).join(', '),
		'content.photos': count(content?.photos, 'photo'),
		'content.quotes': count(content?.quotes, 'quote'),
		'content.contact': [content?.contact?.email, content?.contact?.phone, content?.contact?.note]
			.filter(Boolean)
			.join(' · '),
		'content.techRider': content?.techRider
	};

	return words[field] || 'Empty';
}

/** Conflicts written out for the dashboard. */
export const describeConflicts = (conflicts: EpkConflict[]): EpkConflictView[] =>
	conflicts.map((conflict) => ({
		field: conflict.field,
		label: FIELD_LABELS[conflict.field],
		published: describe(conflict.field, conflict.published),
		previous: describe(conflict.field, conflict.previous),
		draft: describe(conflict.field, conflict.draft)
	}));

/** The fields a submitted conflict form lists, keeping only ones a draft can change. */
export const formFields = (form: FormData, name: string): EpkField[] =>
	form.getAll(name).flatMap((value) => EPK_FIELDS.find((field) => field === value) ?? []);
