# Pages are edited in place

[0001](0001-changes-happen-in-the-dashboard.md) kept every change under `/dashboard` so that a public page would always be a true preview of what visitors see. In practice that split every page in two. Someone fixing a typo in their bio left the page they were reading to find the same words in a form that looked nothing like it, then went back to check the result. Starting an act or a party meant filling in a long form and only seeing the page at the end. Parties and events are where this hurts most, and the apps people already use for them (Partiful above all) build the page as you fill it in.

So every major page type is edited where it's read, by whoever has the capability to change it. A page has an **edit mode** in its own layout: each section's fields sit where its text was, the same size, so nothing moves when it opens. Changes stay on the page until one explicit Save writes them all, or Discard drops them. Saving shows a spinner and then a toast in the same fixed place on every page. Leaving with unsaved changes asks first, and a save that would overwrite someone else's newer change is stopped, as before. Creating something opens its real page empty, in edit mode, as a draft until it's published. **View as visitor** shows the page exactly as a signed-out visitor sees it, which keeps what 0001 wanted from a true preview without hiding the controls.

The controls for the thing on screen live in one **edit bar**, shared by owners and staff. Its staff section holds what only staff can do there, and every staff action from it is logged like anything else staff do. The edit bar acts on one subject, the thing on screen; the staff console does everything else.

The dashboard stays as each account's hub: the inbox, bookings, plans, settings, onboarding, and lists of what's yours or spans several things (people and invites, press kit access, venue rules and approvals). Its forms for a page's content go, and onboarding steps open the page in edit mode instead.

Pages move one type at a time: profile, act, party and event (built this way from the start), press kit (keeping its drafts and Publish from [0002](0002-shared-press-kits-in-root-owned-spaces.md)), venue, mix, show. Projects are skipped, since acts replace them and their edits are review requests.

## Considered options

- Keep editing in the dashboard (0001): the true preview it protected is what View as visitor gives, and the cost was every edit happening away from what it changes.
- Fields always editable for anyone who can edit, with no edit mode (as in Notion): owners would never see their page as others do without switching accounts, a stray click could change a public page, and every page would carry its editing code and its layout for every viewer who can edit.
- Saving each field as it changes: two people editing the same page would trip the stale-write check on almost every keystroke, a half-finished change would be public the moment it was typed, and a page without scripts couldn't save at all. One Save keeps a change whole and works as a plain form.

## Consequences

A page someone can edit isn't cached in their browser, so a save never seems to vanish on reload. Editing code loads only for people who can edit, so visitors' pages stay as light as before.
