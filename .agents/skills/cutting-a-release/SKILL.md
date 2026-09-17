---
name: cutting-a-release
description: Use when releasing this repo — "cut a release", "tag a release", "ship it", "send it", "publish", "bump the version", or any request to get merged work onto GHCR. Covers committing to main, choosing the version, writing the annotated tag, and pushing it to fire the Forgejo release workflow.
---

# Cutting a Release

## Core principle

**The git tag is the only version input.** Nothing in the tree carries a real
version. `helm package` takes both chart fields from the tag, and
`docker/metadata-action` takes the image tags from it. There is nothing to bump
before tagging — the tag *is* the bump.

## Do not ask, just do

Todd's standing preferences for this repo. Following them is not a judgment
call:

- **Commit straight to `main`.** No feature branch, no PR, no "should I?" The
  remote is Forgejo, so `gh` does not work here anyway.
- **"Send it" / "ship it" means commit, push `main`, and tag a release.** All
  of it, in one go. Do not stop after the push and offer the tag as a next step.
- Do not open PRs on this remote unless asked.

## Procedure

```bash
# 1. Preflight — every one of these must pass before tagging
git status --short                      # clean tree
git rev-parse HEAD origin/main          # identical; push main first if not
cd backend && cargo test --locked       # what the release job runs; fix before tagging
cd frontend && npx tsc --noEmit && npm run build

# 2. What is going into this release
git describe --tags --abbrev=0          # last tag
git log --oneline "$(git describe --tags --abbrev=0)"..main

# 3. Tag (annotated, message from a heredoc — never -m, never lightweight)
git tag -a v0.3.2 -F - <<'EOF'
...message, see template below...
EOF

# 4. Fire the release
git push origin v0.3.2
```

Then tell Todd the workflow is building, with the link:
`https://git.greyrock.io/todd/unleashed-voucher-manager/actions`

Pushing the tag publishes the image to
`ghcr.io/greyrock-labs/unleashed-voucher-manager` and the chart to
`oci://ghcr.io/greyrock-labs/helm`. Published packages are not really
retractable, so get the preflight right rather than planning to fix it after.

## Choosing the version

| Bump | When |
|---|---|
| Patch | Fixes, docs, frontend/backend changes with no interface change |
| Minor | Features, or dependency **majors** landing — even when the shipped image is identical |
| Major | The chart's values or the app's interface breaks for an existing deploy |

**Every commit on `main` that the release builds from belongs to a release.**
A dependency bump that cannot change the image still gets a version -- it is a
build input, and the alternative is a deployed commit with no version naming
it. A commit that no build ever reads does not get one: `.agents/`, the README,
CI config. The release job packages `backend/`, `frontend/`, the `Dockerfile`
and `deploy/`; nothing else can reach a published artifact, so nothing else
needs a version. Do not ask about this -- the rule decides it. A `feat(...)!:` commit is a breaking *dependency*,
not necessarily a breaking *chart* — judge the bump by what an existing
deployment has to do, and say so in the notes.

## Tag message template

Title is the bare version. Optional lead paragraph only when the bump level
needs defending. Then sections, in this order, omitting any that are empty:
`Fixed`, `Added`, `Dependencies`, `Documentation`, `Upgrade notes`.

```
v0.3.2

Minor rather than a patch because three dependency majors landed. Nothing in
the chart's values or the app's interface changed, so upgrading from 0.2.2 is
still just a version bump.

Fixed

- <What a user would notice, then why it happened and what the fix actually
  does. One bullet per change, wrapped at 79 columns.>

Dependencies

- <name> <old> -> <new>. <What changed at the call site, and why the behaviour
  is unchanged.>

Upgrade notes

None. No values changed, no chart template surface changed.
```

House style, from the existing tags — match it:

- Written for someone deciding whether to upgrade, not for a changelog robot.
  Explain the *why*, not just the what.
- Say what did **not** change. "Chart-only change", "the image content is
  expected to be identical to 0.3.0", "nothing a guest reads got smaller."
- `Upgrade notes` ends the message and is never omitted. When there is nothing
  to do, say so explicitly: "Upgrading from 0.3.1 needs only the version
  bumped in the OCIRepository."
- Commits on `main` that net to nothing (a change and its revert) get a line
  saying so, not silence.
- Wrap at 79 columns.

## Never

- **Never hand-edit a version to cut a release.** `Chart.yaml` holds `0.0.0`
  in both `version` and `appVersion` deliberately, and the release job fails
  the build if either changed. `frontend/package.json` stays `0.0.0-git`. If
  you catch yourself editing a version field, the tag is what you wanted.
- **Never use a lightweight tag.** Every release tag is annotated and carries
  the notes; `git tag v0.3.2` with no `-a` silently produces a release with no
  story.
- **Never tag a commit that is not on `origin/main`.** The workflow checks out
  the tag from the remote; a local-only commit builds nothing or builds wrong.
- Never re-point or force-push an existing tag. Cut the next patch instead.

## Common mistakes

| Mistake | What happens |
|---|---|
| Branch + PR for a one-line fix | Wasted round trip; Todd will tell you to push to main |
| Stopping after `git push origin main` | Half a release; "send it" included the tag |
| `git tag -m "v0.3.2"` | Release with no notes, and the style is lost |
| Bumping `Chart.yaml` first | Release job fails the placeholder check |
| Tagging with a dirty tree | The tag's tree is not what you tested |
