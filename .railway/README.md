# Railway

`railway.ts` is the website's Railway project (`localnetwork.computer`): the `localnetwork` service, which builds `apps/web/Dockerfile` from `main` once its checks pass, and the `/data` volume it keeps SQLite on. Railway's [Infrastructure as Code](https://docs.railway.com/infrastructure-as-code) reads it through the Railway CLI (5.42.1 or newer) and the `railway` dev dependency at the repository root.

```bash
railway link                  # once: the localnetwork.computer project, production
railway config plan           # what applying the file would change
railway config apply          # apply it, after confirming
```

Secrets stay on Railway: variables written as `preserve()` keep whatever value Railway has. `tools/policy.sh` checks that every app with a Dockerfile has a service here that builds it.

Run `railway` itself, not through `fnm exec`: the SDK finds the CLI through `$_`, which `fnm exec` sets to fnm.
