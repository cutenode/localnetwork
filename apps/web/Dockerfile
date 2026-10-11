# syntax=docker/dockerfile:1.7
# Build from the repo root, which holds the pnpm workspace and lockfile:
#   docker build -f apps/web/Dockerfile .
FROM node:24-bookworm-slim AS base
ENV PNPM_HOME=/pnpm PATH=/pnpm:$PATH CI=true
RUN corepack enable
WORKDIR /app

FROM base AS build
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile --filter localnetwork
COPY lexicons lexicons/
COPY packages/tokens packages/tokens/
# apps/web, and the tools' READMEs and docs for their pages (.dockerignore sends only those).
COPY apps apps/
RUN pnpm --filter localnetwork build
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm --filter localnetwork deploy --prod --ignore-scripts /prod

FROM base AS runtime
ENV NODE_ENV=production PORT=3000 HOST=0.0.0.0 DATABASE_PATH=/data/localnetwork.db
COPY --from=build --chown=node:node /prod/package.json ./
COPY --from=build --chown=node:node /prod/node_modules ./node_modules
COPY --from=build --chown=node:node /app/apps/web/build ./build
RUN mkdir -p /data && chown node:node /data
USER node
VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s CMD node -e "fetch('http://127.0.0.1:3000/healthz').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["node", "build"]
