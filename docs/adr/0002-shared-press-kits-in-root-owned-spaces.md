# Shared press kits live in root-owned spaces

A press kit belongs to its project, and every manager of the project has the same control over it. Only a space's authority can manage its members or delete it, so a press kit's space moved from the manager who created it to the Local Network root account. The root alone writes the published content and the grants there. Managers and grantees are read members.

Managers edit through drafts kept in the project's backstage, a root-owned space whose write members are exactly the current managers. Publishing a draft applies only the fields it changed onto the published copy, and the dashboard asks before overwriting a field someone else published in the meantime. On publish, photos are copied into the root's repo so they outlive a manager leaving. When `project.managers` changes, one sync step updates the members of the project's backstage, its press kit spaces and its open booking threads.

The public half is the `epk` record in the root's repo, at `/projects/[slug]/press-kits/[rkey]`. Old `/epk/<did>/<rkey>` links redirect there. Kits created before this change move over the next time their owner opens the dashboard, keeping their rkey, since only the owner's session can delete the old space.

## Considered options

- Space owned by the manager who created the kit (the old model): co-managers can't grant, revoke, or delete.
- A `managing-app` policy, with the site answering every access check from `project.managers`: there's no membership to sync, but writes are denied whenever the site is unreachable, and nothing else here uses it yet.
- Drafts in the press kit's own space: grantees are read members of that space, so they could read unpublished work.
- Managers writing content straight into the kit's space, with the latest edit winning: that would silently overwrite a co-manager's change.
