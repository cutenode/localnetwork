---
name: linear
description: How this repository plans work in Linear (team LOC) and what an agent may write there without asking. Use when reading, filing or commenting on a LOC issue, picking up work, writing a pull request that closes an issue, or running or coordinating a backlog sweep. For going through the Triage queue, use linear-triage.
---

# Linear for agents

Issues live in the Localnetwork team (`LOC`) of the `localnetwork` Linear workspace, synced both ways with this repository's GitHub issues. [CONTRIBUTING.md](../../../.github/CONTRIBUTING.md#issues-and-planning) has the workflow people follow; this file adds what an agent does. Use the Linear MCP tools (`get_issue`, `list_issues`, `save_comment`, `save_issue`, `save_document`, `save_status_update`).

## Status moves itself

Don't set In Progress, In Review or Done by hand. A pull request that says `Fixes LOC-123` (or `Part of LOC-123`), or a branch named `bnb/loc-123-…`, moves its issue to In Progress when the pull request opens, In Review when a review is requested, and Done when it merges.

## What an agent writes without asking

- **Findings on an issue you worked on**: a new top-level comment (`save_comment` with `issueId`, no `parentId`). It stays in Linear. Never reply in the thread whose first comment is from "GitHub" ("This comment thread is synced to a corresponding GitHub issue"): replies there are posted on the public GitHub issue. A maintainer copies over what should be public.
- **A problem you found that isn't your task**: a new issue in Triage (`save_issue`, team `LOC`, no status) with labels from [labels.yml](../../../.github/labels.yml) and a `relatedTo` link to the issue you were on. New issues sync to GitHub, so they're public: write them for the people who use the tools, with no machine paths, account names or secrets.
- **A sweep batch's report**: a document on the `Backlog sweep` project (`save_document`), titled with the branch name. The coordinator posts one project status update (`save_status_update`) when the sweep ends.

Anything else waits for a maintainer: priority, estimate, project, assignee, labels on an existing issue, closing, cancelling, marking duplicates, and deleting. Put those in your report as proposals.

## Comment on a fixed issue

```markdown
Fixed on `bnb/<branch>`.

Cause: <one or two sentences> ([path.rs:123](https://github.com/cutenode/localnetwork/blob/<sha>/path.rs#L123)).
Fix: <what changed>.
Test: `<test name>` fails without the fix and passes with it.
```

Say "partly fixed" or "not fixable here" plainly, with why (needs hardware, a real account, or a decision).

## Picking up work

Agent work comes from issues with the `agent-ready` label in Backlog or Todo: `list_issues` with `team: "LOC"`, `label: "agent-ready"`, and each of those states. Skip an issue whose "blocked by" relations aren't Done (`get_issue` with `includeRelations`). When several agents run at once, give each one area label (`locallibrary: player`, `web: epk`) so their branches don't touch the same files, and start blocked work only after what blocks it merges.

## Labels

Linear's labels mirror [labels.yml](../../../.github/labels.yml). Linear only gets a label when a synced issue first uses it, so if one you need is missing, create it in the LOC team (`save_issue_label` with `teamId`) with the same name, colour and description.

## Writing

Plain, short sentences. Cite code as `path:line` with a link. No em dashes and no emoji.
