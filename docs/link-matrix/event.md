# Event

In the event page's order ([lib/components/EventPage.svelte](../../apps/web/src/lib/components/EventPage.svelte)). A DJ on the lineup links to their profile; an act links to its page once acts have one.

| Links to | When | On main | Source |
| --- | --- | --- | --- |
| Venue | always | on main | the line under its date [lib/components/EventPage.svelte](../../apps/web/src/lib/components/EventPage.svelte) |
| Party | it belongs to one | on main | the line above its name [lib/components/EventPage.svelte](../../apps/web/src/lib/components/EventPage.svelte) |
| Profiles of your network going or interested | signed in, and they didn't hide it | new page | |
| Each performer on the lineup | always | on main | "Lineup" [lib/components/EventPage.svelte](../../apps/web/src/lib/components/EventPage.svelte) |
| A mix by each performer | the performer has one | new page | |
| The venue's other events | it has others | new page | |
| Community photos' submitters' profiles | a photo was approved | new page | |
| Sets played there | after the night, when sets were published | new page | |
| Ticket sites | it has a ticket link | on main | Tickets [lib/components/EventPage.svelte](../../apps/web/src/lib/components/EventPage.svelte) |
| Its area | always | new page | |
