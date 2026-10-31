# Glossary Map

## Contexts

- [Local Network](./GLOSSARY.md): the website at localnetwork.computer
- [locallibrary](./apps/locallibrary/GLOSSARY.md): the command line tool, player and sidecar, and
  their plugins and accounts

## Relationships

- **locallibrary → Local Network**: locallibrary signs in to some account types through the
  website's token exchange, and publishes a DJ's profile and sets to their Atmosphere account,
  where the website reads them.
- **Grant and access**: the website's grant lets an account read a press kit. locallibrary's
  access lets a plugin use an account. They're unrelated.
