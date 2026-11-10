#!/bin/sh
# Publish a signed release into an appliance's updates directory, where the
# appliance installs its own releases and serves the desktop app's
# (DECISIONS.md, "Appliance updates"). Run by CI on the build box after the
# release's tests pass; the signing key never leaves that machine.
#
#   release/publish.sh appliance VERSION UPDATES_DIR NOTES_FILE PROGRAM_OR_DIR...
#   release/publish.sh desktop   VERSION UPDATES_DIR NOTES_FILE DIST_DIR [CHANNEL]
#
# The desktop app's CHANNEL (DECISIONS.md, "Release channels") is
# production (the default: tagged releases, in desktop/), test (master, in
# desktop/test/) or development (pull requests, in desktop/development/);
# test and development keep only their latest packages; the appliance's
# directory its latest and the one before.
#
# Every package is signed with `tauri signer sign --app-version`, so the
# signature binds the version it was published as. latest.json is written
# last, by rename, so an appliance never reads a release half-written.
#
# A channel only ever moves forward, in version and in content. A release is
# not published when it is older than the one there (a slower run finishing
# after a newer one), nor when it was built without the commit the one there
# was built from (a pull request lacking another's work, or a tag of an
# older commit): an app would lose what it has. On the development channel
# that second refusal does not fail the job: another open pull request may
# merge, and the owner's app stays on the build it has. Test and production
# still fail the job, and so does a development build that lacks master's
# work (a merged pull request's landing commit, or the master a given-up
# build stood on). latest.json records that
# commit: MAGNUS_COMMIT (a pull request's head), or HEAD; the release is the
# checkout at HEAD (for a pull request, its merge with master). It also
# records the pull request (MAGNUS_PR) and `base`, the master commit the
# build stood on. When the build there is an earlier push of the same pull
# request (its commits replaced), or of one closed without merging
# (MAGNUS_GIVEN_UP lists those, as CI finds them), its own work was given up
# on purpose: the release need only hold its base, so master's work still
# never goes back, and a closed pull request cannot stop every later one.
# When it is one that merged (MAGNUS_LANDED lists those, as number=commit,
# the master commit each landed as), its work is in that commit, which holds
# it even when the head does not (a squash merge): the release must hold
# that commit instead.
set -eu

what=$1 version=$2 updates=$3 notes=$4
shift 4
key=${MAGNUS_RELEASE_KEY:-$HOME/.magnus-teams-release/update.key}
commit=${MAGNUS_COMMIT:-$(git rev-parse HEAD)}
pr=${MAGNUS_PR:-}
# A pull request's checkout is its merge with master: master is the first parent.
if [ "$commit" != "$(git rev-parse HEAD)" ]; then base=$(git rev-parse HEAD^1); else base=$commit; fi
[ -r "$key" ] || { echo "no release key at $key" >&2; exit 1; }

sign() {
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" cargo tauri signer sign -f "$key" -p "" --app-version "$version" "$1" >/dev/null
  cat "$1.sig"
}

# Whether VERSION is older than the one $1/latest.json names, by semantic
# version precedence (semver.org, item 11).
older() {
  [ -f "$1/latest.json" ] || return 1
  python3 -I - "$1/latest.json" "$version" <<'EOF'
import json, sys
def key(v):
    core, _, pre = v.split("+")[0].partition("-")
    ids = [(0, int(i), "") if i.isdigit() else (1, 0, i) for i in pre.split(".")] if pre else []
    return [int(n) for n in core.split(".")], not pre, ids
sys.exit(0 if key(sys.argv[2]) < key(json.load(open(sys.argv[1]))["version"]) else 1)
EOF
}

# A field of $1/latest.json (commit, base, pr), if it names one.
built_from() {
  [ -f "$1/latest.json" ] && python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1])).get(sys.argv[2], ""))' "$1/latest.json" "${2:-commit}"
}

# latest.json from its fields, by python so the notes are quoted correctly.
manifest() {
  python3 -I - "$@" <<'EOF'
import json, sys, datetime
out, kind, version, notes, commit, base, pr = sys.argv[1:8]
m = {"version": version, "notes": open(notes).read().strip(), "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"), "commit": commit, "base": base}
if pr:
    m["pr"] = int(pr)
rest = sys.argv[8:]
if kind == "appliance":
    m["file"], m["signature"] = rest[0], rest[1]
else:
    m["platforms"] = {rest[i]: {"file": rest[i + 1], "signature": rest[i + 2]} for i in range(0, len(rest), 3)}
open(out + ".tmp", "w").write(json.dumps(m, indent=2))
EOF
}

case $what in
appliance) dir=$updates/appliance ;;
desktop)
  channel=${2:-production}
  case $channel in
  production) dir=$updates/desktop ;;
  test | development) dir=$updates/desktop/$channel ;;
  *) echo "no channel $channel: production, test or development" >&2; exit 2 ;;
  esac
  ;;
*) echo "publish appliance or desktop" >&2; exit 2 ;;
esac
if older "$dir"; then
  echo "not publishing $what $version: $dir/latest.json names a newer version" >&2
  exit 0
fi
was=$(built_from "$dir" || true)
was_pr=$(built_from "$dir" pr || true)
given_up= landed=
if [ -n "$was_pr" ]; then
  [ "$was_pr" = "$pr" ] && given_up=yes
  for n in $(echo "${MAGNUS_GIVEN_UP:-}" | tr ',' ' '); do [ "$n" = "$was_pr" ] && given_up=yes; done
  for l in $(echo "${MAGNUS_LANDED:-}" | tr ',' ' '); do [ "${l%%=*}" = "$was_pr" ] && landed=${l#*=}; done
fi
if [ -n "$landed" ] && [ -z "$given_up" ]; then
  if ! git merge-base --is-ancestor "$landed" HEAD 2>/dev/null; then
    echo "not publishing $what $version: it was built without $landed, where pull request #$was_pr, whose build $dir/latest.json holds, landed in master; bring this branch up to date with master" >&2
    exit 3
  fi
elif [ -n "$given_up" ]; then
  was_base=$(built_from "$dir" base || true)
  if [ -n "$was_base" ] && ! git merge-base --is-ancestor "$was_base" HEAD 2>/dev/null; then
    echo "not publishing $what $version: it was built without master at $was_base, which $dir/latest.json stood on; bring this branch up to date with master" >&2
    exit 3
  fi
elif [ -n "$was" ] && ! git merge-base --is-ancestor "$was" HEAD 2>/dev/null; then
  echo "not publishing $what $version: it was built without $was, which $dir/latest.json was built from; leaving that build in place so the app does not lose it" >&2
  # Development is the owner's preview of whichever pull request published
  # last. Refusing here used to fail the other open pull request's job, so
  # only one could be green. The app still does not move. A caller that
  # named where some pull request landed, and named the wrong one, still
  # fails: that is not an open pull request sitting beside this one.
  if [ "${channel:-}" = development ] && [ -z "${MAGNUS_LANDED:-}" ]; then exit 0; fi
  exit 3
fi

case $what in
appliance)
  previous=$( [ -f "$dir/latest.json" ] && python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["file"])' "$dir/latest.json" || true)
  mkdir -p "$dir"
  stage=$(mktemp -d)
  trap 'rm -rf "$stage"' EXIT
  for p in "$@"; do cp -r "$p" "$stage/"; done
  echo "$version" > "$stage/VERSION"
  file=magnus-appliance-$version.tar.gz
  tar -C "$stage" -czf "$dir/$file" .
  manifest "$dir/latest.json" appliance "$version" "$notes" "$commit" "$base" "$pr" "$file" "$(sign "$dir/$file")"
  ;;
desktop)
  dist=$1
  mkdir -p "$dir"
  linux=Magnus_${version}_amd64.AppImage
  windows=Magnus_${version}_x64-setup.exe
  cp "$dist/$linux" "$dist/$windows" "$dir/"
  manifest "$dir/latest.json" desktop "$version" "$notes" "$commit" "$base" "$pr" \
    linux-x86_64 "$linux" "$(sign "$dir/$linux")" \
    windows-x86_64 "$windows" "$(sign "$dir/$windows")"
  ;;
esac
mv "$dir/latest.json.tmp" "$dir/latest.json"
# The appliance's packages: this one and the one before it; the installed
# releases themselves are kept by the appliance (`<dir>.prev`).
if [ "$what" = appliance ]; then
  for f in "$dir"/magnus-appliance-*; do
    case $f in "$dir/$file" | "$dir/$file.sig" | "$dir/$previous" | "$dir/$previous.sig") ;; *) rm -f "$f" ;; esac
  done
fi
if [ "$what" = desktop ] && [ "$channel" != production ]; then
  for f in "$dir"/Magnus_*; do
    case $f in "$dir/$linux" | "$dir/$windows" | "$dir/$linux.sig" | "$dir/$windows.sig") ;; *) rm -f "$f" ;; esac
  done
fi
echo "published $what $version to $dir"
