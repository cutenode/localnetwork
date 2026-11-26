# Act

| Links to | When | On main | Source |
| --- | --- | --- | --- |
| Members' profiles | members who aren't hidden | on main | the handle beside "How they play" [routes/projects/\[slug\]/+page.svelte](../../apps/web/src/routes/projects/%5Bslug%5D/%2Bpage.svelte) |
| Managers' profiles | it has managers | on main | "Managed by" [routes/projects/\[slug\]/+page.svelte:186](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/projects/%5Bslug%5D/%2Bpage.svelte#L186) |
| Its press kits | it has press kits | on main | [routes/projects/\[slug\]/+page.svelte:80](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/projects/%5Bslug%5D/%2Bpage.svelte#L80) |
| Its mixes | it has mixes | on main | only those with a tracklist, to `/projects/<slug>/mixes/<n>` [routes/projects/\[slug\]/+page.svelte:124](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/projects/%5Bslug%5D/%2Bpage.svelte#L124) |
| Its sets | members published sets | on main | each set's name [lib/components/DjSets.svelte](../../apps/web/src/lib/components/DjSets.svelte) |
| Events it's on the lineup for | it has credits | new page | shows render inline [routes/projects/\[slug\]/+page.svelte:113-119](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/projects/%5Bslug%5D/%2Bpage.svelte#L113-L119) |
| Parties and venues it played | its events name them | new page | |
| Artists in its sets | members published sets | on main | [lib/components/DjSets.svelte:125](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/lib/components/DjSets.svelte#L125) |
| Tracks in its sets | the tracks flag is on | new page | |
| Followers and Following | always | new page | |
| Send an inquiry | always | on main | signed in only [routes/projects/\[slug\]/+page.svelte:70](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/projects/%5Bslug%5D/%2Bpage.svelte#L70) |
| locallibrary | members published stats | on main | "From locallibrary" under How they play [lib/components/DjStats.svelte](../../apps/web/src/lib/components/DjStats.svelte) |
