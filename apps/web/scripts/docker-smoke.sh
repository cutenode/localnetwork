#!/usr/bin/env bash
# Runs the web image the way it's deployed, with an empty volume at /data and its configuration in
# the environment, against the local network `pnpm dev:network` starts (the one the e2e tests use),
# then fetches a few pages. From the repo root:
#
#   docker build -f apps/web/Dockerfile -t localnetwork . && apps/web/scripts/docker-smoke.sh localnetwork
set -euo pipefail

image=${1:?usage: docker-smoke.sh <image>}
port=3000
name=localnetwork-smoke-$$
work=$(mktemp -d)
network=

cleanup() {
	local status=$?
	if ((status != 0)) && docker container inspect "$name" >/dev/null 2>&1; then
		echo "--- container logs" >&2
		docker logs "$name" >&2 || true
	fi
	docker rm --force --volumes "$name" >/dev/null 2>&1 || true
	if [[ -n $network ]]; then
		kill "$network" 2>/dev/null || true
		wait "$network" || true
	fi
	rm -rf "$work"
}
trap cleanup EXIT

cd "$(dirname "$0")/.."

# The network writes the app's configuration to DEV_ENV_FILE once its accounts, projects and
# HappyView are ready.
PORT=$port DEV_PLC_PORT=2882 DEV_PDS_PORT=2883 DEV_DB_PATH="$work/network.db" DEV_ENV_FILE="$work/env" \
	./node_modules/.bin/tsx scripts/dev-network.ts >"$work/network.log" 2>&1 &
network=$!

for _ in $(seq 180); do
	[[ -s $work/env ]] && break
	if ! kill -0 "$network" 2>/dev/null; then
		cat "$work/network.log" >&2
		exit 1
	fi
	sleep 1
done
[[ -s $work/env ]] || { echo "the network didn't start in 3 minutes" >&2; exit 1; }

# Host networking, so the app reaches the network on 127.0.0.1 as its configuration says. The
# database goes on the volume, as in production; the SSRF guard has to allow the loopback PDS.
docker run --detach --name "$name" --network host --env-file "$work/env" \
	--env DATABASE_PATH=/data/localnetwork.db --env ALLOW_PRIVATE_NETWORK=true \
	--mount type=volume,target=/data "$image" >/dev/null

for _ in $(seq 60); do
	curl --silent --fail "http://127.0.0.1:$port/healthz" >/dev/null && break
	sleep 1
done

# Each path answers 200 with this text in it.
expect() {
	local body
	body=$(curl --silent --show-error --fail "http://127.0.0.1:$port$1")
	if ! grep --quiet --fixed-strings -- "$2" <<<"$body"; then
		echo "$1 doesn't contain \"$2\"" >&2
		exit 1
	fi
	echo "ok  $1"
}

expect /healthz '"happyView":"ok"'
# The headline's first pair, which the server renders; its words are in headline.json.
expect / "$(node --print "const { verbs, nouns } = require('./src/lib/headline.json'); \`\${verbs[0]} your \${nouns[0]}.\`")"
expect /projects/halflight 'Halflight'
# The tools' guides come from apps/<tool>/, which only the image's build context brings in.
expect /tools/locallibrary/docs/install 'Installing locallibrary'
expect /robots.txt 'User-agent'
# The image runs scripts/db-backup.ts from source. With no bucket set it stops at asking for one,
# which it only gets to once every file it imports is in the image.
backup=$(docker exec "$name" node scripts/db-backup.ts list 2>&1 || true)
if ! grep --quiet --fixed-strings 'Set BACKUP_S3_BUCKET' <<<"$backup"; then
	echo "scripts/db-backup.ts doesn't run in the image: $backup" >&2
	exit 1
fi
echo "ok  scripts/db-backup.ts"
