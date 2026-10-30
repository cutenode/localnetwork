---
name: linear-triage
description: Go through the LOC team's Triage queue in Linear and propose priority, labels, project, relations and whether each issue is agent-ready, for approval before anything changes. Use when asked to triage, clear Triage, or plan what agents can pick up next.
---

# Triage the LOC queue

Load the `linear` skill first. Nothing in Linear changes until a maintainer picks rows.

## 1. Gather

`list_issues` with `team: "LOC"`, `state: "triage"`. For each issue:

- Read it and its comments (`get_issue`, `list_comments`).
- Look for duplicates and related work: `list_issues` with `query` on its key terms, `includeArchived: true`, across every status.
- Check whether `main` already fixed it (`git log origin/main`, `gh pr list --state merged --search "<terms>"`, the code it names).

## 2. Propose

One row per issue:

- **Status**: Backlog, Todo, Canceled (with why), or Duplicate of LOC-n.
- **Priority**: 1 urgent to 4 low. Urgent only for data loss, security, or a broken release.
- **Estimate**, if the team has estimates on.
- **Labels**: a tool or `<tool>: <area>` label plus `bug` or `enhancement`, all from [labels.yml](../../../.github/labels.yml).
- **Project**: `locallibrary v0.1.0` only when the release can't ship without it.
- **Relations**: blocked by, related to.
- **agent-ready**: yes only when the issue names the outcome, where in the code, and how to test it, and needs no hardware, real account or decision. If it's close, propose the missing sentences for its description too.

Every row carries its evidence: links to the duplicate, the merged pull request, or the code line.

## 3. Ask

Publish the rows as an artifact table with a checkbox per row, saved in the artifact's shared database. Wait for "do the checked boxes" or pasted rows.

## 4. Apply

For the picked rows only, one `save_issue` per issue: `state`, `priority`, `estimate`, `project`, `addLabels` (never `labels`, which replaces the whole set), `blockedBy`, `relatedTo` or `duplicateOf`, and `agent-ready` in `addLabels` where proposed. Then list what changed, with LOC ids.
