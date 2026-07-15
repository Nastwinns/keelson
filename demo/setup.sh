#!/usr/bin/env sh
# Builds a throwaway multi-repo workspace for the VHS demos (demo/*.tape).
# Sourced by the tapes so the `cd` into the workspace persists.

DEMO_LAB="/tmp/haw-demo"
rm -rf "$DEMO_LAB"
export GIT_AUTHOR_NAME="Keelson Demo" GIT_AUTHOR_EMAIL="demo@keelson.dev"
export GIT_COMMITTER_NAME="Keelson Demo" GIT_COMMITTER_EMAIL="demo@keelson.dev"

for repo in kernel hal app-mqtt; do
    mkdir -p "$DEMO_LAB/$repo"
    cd "$DEMO_LAB/$repo" || exit 1
    git init -q -b main
    git config user.email demo@keelson.dev
    git config user.name "Keelson Demo"
    echo "$repo sources" > README.md
    git add . && git commit -qm "init $repo"
done

mkdir -p "$DEMO_LAB/gateway"
cd "$DEMO_LAB/gateway" || exit 1
cat > haw.toml <<MANIFEST
[repo.kernel]
url = "$DEMO_LAB/kernel"
rev = "main"
groups = ["firmware"]

[repo.hal]
url = "$DEMO_LAB/hal"
rev = "main"
groups = ["firmware"]

[repo.app-mqtt]
url = "$DEMO_LAB/app-mqtt"
rev = "main"

[stack.gateway]
repos = ["kernel", "hal", "app-mqtt"]

[stack.sensor-node]
repos = ["kernel", "hal"]
MANIFEST
clear
