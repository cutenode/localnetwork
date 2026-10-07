import type { ProjectDetails, ProjectProfile, ProjectStatus, ProjectView, Viewer } from '#lib/types.ts';
import { type Core, ForbiddenError, UserError } from '../core';
import type { ProjectService } from './projects';
import { actorRole } from './requests';

/** Direct project edits by staff. These skip the request queue but are fully audited. */
export class ProjectAdmin {
	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService
	) {}

	private async requireAdmin(
		actor: Viewer,
		action: 'project.create' | 'project.update' | 'project.managers',
		subject?: string
	) {
		if (actor.isAdmin) return;
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action,
			subject,
			subjectKind: 'project',
			summary: 'Denied: only admins can edit projects directly',
			outcome: 'denied'
		});
		throw new ForbiddenError('Only admins can edit projects directly');
	}

	private async withAvatar(
		profile: ProjectProfile,
		avatar?: { bytes: Uint8Array; mimeType: string }
	): Promise<ProjectProfile> {
		if (!avatar) return profile;
		const root = await this.core.root();

		return { ...profile, avatar: await root.uploadBlob(avatar.bytes, avatar.mimeType) };
	}

	/** Uploads new mix tracklists (aligned with `profile.mixes`) to the root's repo. */
	private async withTracklists(
		profile: ProjectProfile,
		tracklists?: ({ bytes: Uint8Array; mimeType: string } | undefined)[]
	): Promise<ProjectProfile> {
		if (!profile.mixes || !tracklists?.some(Boolean)) return profile;
		const root = await this.core.root();
		const mixes = [...profile.mixes];

		for (const [index, file] of tracklists.entries()) {
			if (file && mixes[index])
				mixes[index] = { ...mixes[index], tracklist: await root.uploadBlob(file.bytes, file.mimeType) };
		}

		return { ...profile, mixes };
	}

	async create(input: {
		actor: Viewer;
		profile: ProjectProfile;
		details?: ProjectDetails;
		managers?: string[];
		avatar?: { bytes: Uint8Array; mimeType: string };
		tracklists?: ({ bytes: Uint8Array; mimeType: string } | undefined)[];
	}): Promise<ProjectView> {
		await this.requireAdmin(input.actor, 'project.create');

		const { project, changes } = await this.projects.publish({
			profile: await this.withTracklists(await this.withAvatar(input.profile, input.avatar), input.tracklists),
			managers: input.managers ?? [],
			status: 'active',
			createdBy: input.actor.did,
			details: input.details && Object.keys(input.details).length ? input.details : undefined
		});

		await this.core.audit.record({
			actor: input.actor.did,
			actorRole: 'admin',
			action: 'project.create',
			subject: project.uri,
			subjectKind: 'project',
			summary: `Added ${project.profile.name}`,
			changes
		});

		return project;
	}

	async update(input: {
		actor: Viewer;
		project: ProjectView;
		profile: ProjectProfile;
		details?: ProjectDetails;
		avatar?: { bytes: Uint8Array; mimeType: string };
		removeAvatar?: boolean;
		tracklists?: ({ bytes: Uint8Array; mimeType: string } | undefined)[];
	}): Promise<ProjectView> {
		await this.requireAdmin(input.actor, 'project.update', input.project.uri);
		let profile = await this.withTracklists(await this.withAvatar(input.profile, input.avatar), input.tracklists);

		if (!input.avatar && !input.removeAvatar && input.project.profile.avatar)
			profile = { ...profile, avatar: input.project.profile.avatar };

		const { project, changes } = await this.projects.publish({
			existing: input.project,
			profile,
			managers: input.project.managers,
			status: input.project.status,
			createdBy: input.project.createdBy,
			details: input.details
		});

		await this.core.audit.record({
			actor: input.actor.did,
			actorRole: 'admin',
			action: 'project.update',
			subject: project.uri,
			subjectKind: 'project',
			summary: `Edited ${project.profile.name} (revision ${project.revision})`,
			changes
		});

		return project;
	}

	async setStatus(input: {
		actor: Viewer;
		project: ProjectView;
		status: ProjectStatus;
		reason?: string;
	}): Promise<ProjectView> {
		if (!input.actor.isStaff) throw new ForbiddenError();

		if (input.status === 'archived' && !input.actor.isAdmin)
			throw new ForbiddenError('Only admins can archive projects');

		if (input.project.status === input.status) return input.project;

		const { project, changes } = await this.projects.publish({
			existing: input.project,
			profile: input.project.profile,
			managers: input.project.managers,
			status: input.status,
			createdBy: input.project.createdBy
		});

		await this.core.audit.record({
			actor: input.actor.did,
			actorRole: actorRole(input.actor),
			action: 'project.status',
			subject: project.uri,
			subjectKind: 'project',
			summary: `Set ${project.profile.name} to ${input.status}`,
			changes,
			reason: input.reason
		});

		return project;
	}

	async setManagers(input: { actor: Viewer; project: ProjectView; managers: string[] }): Promise<ProjectView> {
		await this.requireAdmin(input.actor, 'project.managers', input.project.uri);
		const managers = [...new Set(input.managers)];

		if (managers.some((did) => !did.startsWith('did:'))) throw new UserError('Managers must be DIDs');

		if (managers.length > 10) throw new UserError('A project can have at most 10 managers');

		const { project, changes } = await this.projects.publish({
			existing: input.project,
			profile: input.project.profile,
			managers,
			status: input.project.status,
			createdBy: input.project.createdBy
		});

		await this.core.audit.record({
			actor: input.actor.did,
			actorRole: 'admin',
			action: 'project.managers',
			subject: project.uri,
			subjectKind: 'project',
			summary: `Changed managers of ${project.profile.name}`,
			changes
		});

		return project;
	}
}
