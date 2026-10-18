# syntax=docker/dockerfile:1.7
# Build from the repo root, which holds the pnpm workspace and lockfile:
#   docker build -f apps/web/Dockerfile .
FROM node:24-bookworm-slim AS base
ENV PNPM_HOME=/pnpm PATH=/pnpm:$PATH CI=true
RUN corepack enable
WORKDIR /app

FROM base AS build
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
# pnpm checks every patch pnpm-workspace.yaml lists, even for packages this install skips.
COPY patches patches/
COPY apps/web/package.json apps/web/
# No cache mount: Railway only accepts one whose id names its service, and the layer cache already
# skips this step while the lockfile and manifests are unchanged.
RUN pnpm install --frozen-lockfile --filter localnetwork
COPY lexicons lexicons/
COPY packages/tokens packages/tokens/
# apps/web, and the tools' READMEs and docs for their pages (.dockerignore sends only those).
COPY apps apps/
RUN pnpm --filter localnetwork build
RUN pnpm --filter localnetwork deploy --prod --ignore-scripts /prod

FROM base AS runtime
# Forms carry photos (press kit photos up to 5 MB each) and tracklists; adapter-node allows 512 KB by default.
ENV NODE_ENV=production PORT=3000 HOST=0.0.0.0 DATABASE_PATH=/data/localnetwork.db BODY_SIZE_LIMIT=40M
COPY --from=build --chown=node:node /prod/package.json ./
COPY --from=build --chown=node:node /prod/node_modules ./node_modules
COPY --from=build --chown=node:node /app/apps/web/build ./build
# The database lives on a volume mounted at /data (`-v` locally, a Railway volume in production);
# Railway refuses a `VOLUME` line.
RUN mkdir -p /data && chown node:node /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s CMD node -e "fetch('http://127.0.0.1:3000/healthz').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
# Railway mounts its volume owned by root, so the container starts as root, hands /data to the node
# user, and runs the app as node. Started as another user (`docker run --user`), it runs as that.
ENTRYPOINT ["sh", "-c", "if [ \"$(id -u)\" = 0 ]; then chown -R node:node /data && exec setpriv --reuid=node --regid=node --init-groups \"$@\"; fi; exec \"$@\"", "--"]
CMD ["node", "build"]
