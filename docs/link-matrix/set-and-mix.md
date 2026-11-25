# Set and mix

A mix is a set's page with the player. On main the closest page is a mix's tracklist report, `/projects/<slug>/mixes/<n>`.

| Links to | When | On main | Source |
| --- | --- | --- | --- |
| Credited performers | always | on main | back to the act [routes/projects/\[slug\]/mixes/\[n\]/+page.svelte:18](https://github.com/cutenode/localnetwork/blob/fe5cadefac6a3ab54ab995e1e4786b1fd93fc598/apps/web/src/routes/projects/%5Bslug%5D/mixes/%5Bn%5D/%2Bpage.svelte#L18); missing to members' profiles |
| The set itself, from a mix | the mix links a set | on main | "Set:" [lib/components/LinkedSet.svelte](../../apps/web/src/lib/components/LinkedSet.svelte) |
| The show and event it was played at | it was played at an event | new page | |
| Each tracklist row's artist | the row names one | on main | on the tracklist page [lib/components/mix/MixTracklist.svelte](../../apps/web/src/lib/components/mix/MixTracklist.svelte) |
| Each tracklist row's track | the tracks flag is on | new page | |
| Press kits | the performer has press kits | missing | |
