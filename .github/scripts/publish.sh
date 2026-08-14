#!/usr/bin/env bash
#
# Publish each crate in the derived order, waiting for the crates.io index to
# show each one before starting the next. Reads ORDER (space-separated crate
# names, dependencies first) and VERSION from the environment.
#
# CARGO_REGISTRY_TOKEN is consumed by cargo directly. It is never read, printed
# or passed on the command line here.
#
# Idempotent on purpose. Partial publication is the realistic failure mode —
# the first crate succeeds and the third fails — and a re-run has to finish the
# job rather than abort on "crate version already exists". So every crate asks
# the index what is already there instead of assuming, and a non-zero exit from
# cargo is a question rather than a verdict.
#
# Why a loop rather than `cargo publish --workspace`, which orders and waits by
# itself: the workspace publish aborts when a version is already on the
# registry, which is precisely the state a retry starts from. It is the better
# tool right up until the moment this automation exists for.
#
# See docs/publishing.md.
set -euo pipefail

: "${ORDER:?ORDER must be set to the space-separated publish order}"
: "${VERSION:?VERSION must be set to the workspace version}"

# Sparse-index layout: crates.io shards by name length, then by prefix.
index_url() {
    local name=$1
    case ${#name} in
        1) printf 'https://index.crates.io/1/%s\n' "$name" ;;
        2) printf 'https://index.crates.io/2/%s\n' "$name" ;;
        3) printf 'https://index.crates.io/3/%s/%s\n' "${name:0:1}" "$name" ;;
        *) printf 'https://index.crates.io/%s/%s/%s\n' "${name:0:2}" "${name:2:2}" "$name" ;;
    esac
}

# The index is what a later `cargo publish` resolves its dependencies against,
# so the index — not the web API, and certainly not a fixed sleep — is the
# thing worth waiting on. A 404 means the crate has never been published.
on_index() {
    local name=$1 body
    body=$(curl -sSf --max-time 30 "$(index_url "$name")" 2>/dev/null) || return 1
    jq -e -s --arg v "$VERSION" 'map(.vers) | index($v) != null' >/dev/null <<<"$body"
}

wait_for_index() {
    local name=$1 attempts=$2 attempt
    for ((attempt = 1; attempt <= attempts; attempt++)); do
        if on_index "$name"; then
            echo "  $name $VERSION is on the index (check $attempt of $attempts)"
            return 0
        fi
        sleep 10
    done
    return 1
}

for crate in $ORDER; do
    echo "::group::$crate $VERSION"

    if on_index "$crate"; then
        echo "  already on crates.io — nothing to do"
        echo "::endgroup::"
        continue
    fi

    set +e
    output=$(cargo publish -p "$crate" 2>&1)
    status=$?
    set -e
    printf '%s\n' "$output"

    if [ $status -ne 0 ]; then
        # Let the index be the judge. That collapses three cases into one: an
        # upload that raced a concurrent run, an upload that succeeded but
        # timed out while cargo waited for it to be indexed, and a genuine
        # failure. Only the last leaves the version off the index. Cargo has
        # already done its own waiting by this point, so this check is short.
        echo "  cargo publish exited $status — asking the index whether it landed anyway"
        if ! wait_for_index "$crate" 6; then
            echo "::error::publishing $crate $VERSION failed and it is not on the index"
            echo "::endgroup::"
            exit $status
        fi
        echo "::notice::$crate $VERSION was already published despite exit $status — continuing"
    elif ! wait_for_index "$crate" 60; then
        echo "::error::$crate $VERSION was uploaded but has not appeared on the index after 10 minutes; the next crate cannot resolve it"
        echo "::endgroup::"
        exit 1
    fi

    echo "::endgroup::"
done

echo "published (or confirmed already published): $ORDER"
