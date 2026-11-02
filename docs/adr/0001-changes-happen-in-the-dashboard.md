# Changes happen in the dashboard, public pages only read

Every change to what a public page shows (a project's listing, its shows, its press kits and their grants, booking threads, requests) happens under `/dashboard`. Staff work happens under `/admin`, which uses the same shell with its own navigation and role check. Public pages can show more to viewers who are allowed to read more, like a press kit's private part and the project's booking details for people with a grant. They never show controls for changing it, apart from a single "Manage" link for the project's managers and staff. There's one exception: following a project and saving it to your planner stay as inline toggles, because they only change the viewer's own data.

Before this, owner controls were scattered over the public pages and private pages sat at several top-level paths. We moved the private routes without redirects, since no notification or outside link pointed at them. Public URLs kept their paths, apart from press kits (see [0002](0002-shared-press-kits-in-root-owned-spaces.md)).

## Considered options

- An `app.` subdomain for the dashboard: rejected because it needs a second OAuth client metadata document and cross-origin sessions, with nothing gained.
- Inline edit controls on public pages: rejected so that a public page is always a true preview of what visitors see.
