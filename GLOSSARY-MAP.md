# Glossary Map

## Contexts

- [Local Network](./GLOSSARY.md): the website at localnetwork.computer
- [locallibrary](./apps/locallibrary/GLOSSARY.md): the command line tool, player and sidecar, and
  their plugins and accounts
- [localtoolbox](./apps/localtoolbox/GLOSSARY.md): the Chrome extension

## Relationships

- **locallibrary → Local Network**: locallibrary signs in to some account types through the
  website's token exchange, and publishes a DJ's profile and sets to their Atmosphere account,
  where the website reads them.
- **Grant and access**: the website's grant lets an account read a press kit. locallibrary's
  access lets a plugin use an account. They're unrelated.
- **Plan and plan**: the website's plan is an event someone's going to or a show they're on.
  locallibrary's plan is what a command will change, shown before it's applied. They're unrelated.
