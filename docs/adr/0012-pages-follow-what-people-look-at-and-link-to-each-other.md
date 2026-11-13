# Pages follow what people look at, and link to each other

The website's pages were added one at a time. At [`fe5cadef`](https://github.com/cutenode/localnetwork/tree/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598) it has 67 page routes. The header is Find DJs, Shows, Charts and Tools for everyone ([lib/nav.ts:12-17](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/lib/nav.ts#L12-L17)), and once signed in, everything of your own sits behind one Dashboard link with no unread count; the account menu holds only the staff console and Sign out ([lib/nav.ts:30-43](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/lib/nav.ts#L30-L43)). Sets, shows, follows, acts, parties and venues have no page, so nothing can link to them. The one link to a set goes to an `at://` URI a browser can't open ([lib/components/LinkedSet.svelte:17](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/lib/components/LinkedSet.svelte#L17)). A listed DJ's search result goes to `/@handle`, which has no inquiry, Follow, shows or press kits, and the inquiry form 404s without a project ([routes/dashboard/bookings/new/+page.server.ts:28](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/dashboard/bookings/new/%2Bpage.server.ts#L28)). The inbox only hears about bookings ([lib/server/services/bookings.ts:736-738](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/lib/server/services/bookings.ts#L736-L738)), and the dashboard never mentions locallibrary. [The link matrix](../link-matrix.md) lists every link that exists today and every one that's missing.

This decides how the site is organised and linked: around what people come to look at, with every page linking to every related page the data allows.

## The site map

There's one top-level navigation, the same for everyone. Roles change your own dashboard and home page, never the site map. On a desktop the header has Events, DJs, Mixes and Search on the left, with Charts under Mixes, and on the right the inbox with its unread count and an avatar menu. On a phone a bottom bar holds Home, Events, Search, Inbox and You. Signed out, the right side is a Sign in button, and Follow and RSVP ask people to sign in. The signed-out home page shows upcoming events in your area, then DJs and mixes from there, and pushes people to sign in. Signed in, home is a feed of your network in date order, with a filter by kind, beside a For you tab.

Every noun in the [glossary](../../GLOSSARY.md) that people link to gets a public page: profile, act, party, venue, event, set, mix, press kit, artist and track. Acts, parties and venues are at `/act/<slug>`, `/party/<slug>` and `/venue/<slug>`, an event is under its party at `/party/<slug>/<event>`, or at `/@handle/events/<rkey>` under the account that posted it when it has no party, and a set is under its DJ at `/@handle/sets/<rkey>`. That one set page shows its tracklist, the show and event it was played at, and the player when the set is a mix. `/mixes` lists the sets that are mixes. A track's page shows its artists, the sets that played it and the DJs who played them, and every tracklist row links its track and artist. Tracks stay behind a flag while the dataset is empty and launch once it's public. Labels wait until there's label data.

Events are browsed by area: cities, from each venue's location, at `/events/<country>/<city>`, with genre and date as URL parameters. Your home area comes from your profile or from onboarding. Search is one box over every kind, with a tab per kind, ranked by your network and area, at `/search?q=&type=`.

Profiles show tabs by type, each hidden when it's empty and linkable as `?tab=`. Upcoming and past are a filter inside a tab. Every profile, act, party and venue has Followers and Following pages, with a kind filter. An event page runs in this order: title, date and time, venue and party, Going and Interested with how many of your network are going, the lineup with a Follow button and a mix for each performer, the party, the venue with its other events, host pictures then community photos, and the sets played there once the night is over. Each section is left out when its data isn't there, and events link out to the ticket sites that sell them.

## Links

[The link matrix](../link-matrix.md) is the list of which pages each kind of page links to, as tables and diagrams. Each page type gets a test that fails when a page leaves out a link the matrix asks for and the data is there.

## Acting as someone

There's one dashboard for each account, plus a switcher for which act, party or venue you're acting for. The avatar menu lists every identity you can act as, the header shows who you're acting as, and the dashboard's sections follow it, with the onboarding checklist first until it's done. The switcher uses as much of each identity's Atmosphere profile as it can. Every act, party and venue has a square avatar.

Acts, parties and venues can link an Atmosphere account. For now that's an account they bring. Whether the site runs its own PDS and gives each of them an account is decided after the locallibrary release, with a design doc on key custody and on several people using one account. Verification extends to acts, managers, parties and venues, and is always a record.

## Onboarding

A short flow on first sign-in, then a checklist on the dashboard home with one step per piece of data, each saying what filling it in gets you. A flag that turns a feature on for someone adds that feature's setup steps to the checklist, marked new, so later features ask for the data they need the same way. Onboarding starts from the Bluesky profile: name, avatar, banner, bio, link records and pronouns.

## Events and people from elsewhere

Events and RSVPs made in other AT Protocol apps come in without anyone accepting each one, but only when they name a Local Network venue, party or performer. Everything else is ignored. A venue or party added later pulls in its past events. Which outside events count as nightlife and music, beyond that tie, is still open.

Attendance from those RSVPs feeds recommendations and shows on your own dashboard. "Went to" is on your public profile by default, and you can turn it off. Going and Interested show on the event page and to your followers by default, and can be hidden for one event or for all of them. On a private event they show only to people who can open the event.

Host pictures use the event's `media` field from the community calendar lexicon (roles thumbnail, poster, flyer and header). A community photo is a record in the submitter's own repo that points at the event, party or venue, and it shows once someone running that approves it, credited to the submitter and with their consent. Anyone running the party or venue moderates its community photos, and so do staff in the staff console. grain.social albums that reference an event are read the same way. The photo record goes to the community lexicon effort as a proposal.

A lineup credit shows on the performer's page at once, marked unconfirmed until they confirm it from their inbox, and they can remove it. Their followers hear about it once it's confirmed.

## Inbox and plans

Every inbox item opens what it's about. Inquiries and messages from strangers wait under a Requests tab, so asking staff to list or confirm something is now called a review request. The inbox also takes claim and listing decisions, press kit access requests, lineup credits to confirm and community photos to approve.

A plan is an event you're going to or interested in, or a show you're on the lineup for. "Your plans" is one list on the dashboard, and the planner stays where promoters work out shows. Plans can be kept in a space by people who don't want them public. The dashboard shows your network's activity, and when you have no plans in the next two weeks it suggests events in your area that your network is going to, then events from what you follow, each with Going, Interested and Invite, beside a short feed of your network. Invite goes through the inbox, or as a Bluesky DM to people who opted into those.

## What comes first

Before the locallibrary release: navigation and linking for the pages that exist, the pages locallibrary needs, and the first onboarding steps. "Get locallibrary" goes in the avatar menu, on a dashboard card, and on the DJs page and every DJ's profile. Events and the social pages follow their own plan behind flags.

## Consequences

- Most of the work is new pages: acts, parties, venues, events, sets, tracks, areas, search and the follow lists. Each lands with its row of the link matrix and its test.
- `/projects/<slug>` is still the act page on main, so the matrix lists today's links under it until acts get their own pages.
- Charts moves under Mixes and Tools leaves the header for the footer, where it already is.
- RSVP buttons on a public event page fall under [0001](0001-changes-happen-in-the-dashboard.md)'s exception for controls that only change the viewer's own data, like Follow and Save.
- The inbox grows from booking events to every decision someone has to make, so notification kinds stop being bookings-only.
