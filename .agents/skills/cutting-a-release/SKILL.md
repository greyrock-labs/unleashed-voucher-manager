---
name: cutting-a-release
description: 'Use when releasing this repo: "cut a release", "tag a release", "ship it", "send it", "publish", "bump the version", or any request to get merged work onto GHCR. Covers committing to main, choosing the version, writing the signed annotated tag, and pushing it to fire the Forgejo release workflow.'
---

# Cutting a release

## Core principle

**The git tag is the only version input.** Nothing in the tree carries a real
version: `backend/Cargo.toml` and `frontend/package.json` both stay
`0.0.0-git`, and `docker/metadata-action` takes the image tags from the git
tag. There is nothing to bump before tagging; the tag is the bump.

## Standing preferences

- **Commit straight to `main`.** No feature branch or pull request unless
  asked.
- **"Send it" or "ship it" means commit, push `main`, and tag a release**, all
  in one go. Do not stop after the push and offer the tag as a next step.

## Procedure

```bash
# 1. Preflight: every one of these must pass before tagging
git status --short                      # clean tree
git rev-parse HEAD origin/main          # identical; push main first if not
(cd backend && cargo test --locked)     # what the release job runs
(cd frontend && npx tsc --noEmit && npm test && npm run build)
# ...and CI on origin/main must have passed for HEAD. It runs on Node 24 and
# can fail where a newer local toolchain passes. Check with the Forgejo API:
#   /api/v1/repos/<owner>/<repo>/actions/tasks, filtered on head_sha

# 2. What is going into this release
git describe --tags --abbrev=0          # last tag
git log --oneline "$(git describe --tags --abbrev=0)"..main

# 3. Tag: annotated and signed, message from a heredoc
git tag -s v1.2.3 -F - <<'MSG'
...message, see the template below...
MSG

# 4. Fire the release
git push origin v1.2.3
```

Pushing the tag runs `.forgejo/workflows/release.yaml`, which tests the
backend and publishes the image to
`ghcr.io/greyrock-labs/unleashed-voucher-manager`. Published images are not
really retractable, so get the preflight right rather than planning to fix it
afterwards.

## Choosing the version

| Bump | When |
|---|---|
| Patch | Fixes, and changes with no effect on configuration or behaviour a deployment relies on |
| Minor | Features, or dependency major versions landing, even when the image behaves the same |
| Major | An existing deployment must change something: an environment variable renamed or removed, a route changed, a default changed |

**Every commit that changes code belongs to a release.** A dependency bump
gets a version too; otherwise a deployed commit has no version naming it. A
commit that changes only documentation, or only a comment in a code file,
does not need a release.

## Tag message template

The title is the bare version. Add a short lead paragraph only when the bump
level needs explaining. Then these sections in this order, leaving out any
that are empty: `Fixed`, `Added`, `Changed`, `Dependencies`, `Upgrade notes`.

```
v1.2.3

Fixed

- <What a user would notice, then why it happened and what the fix does.
  One bullet per change, wrapped at 79 columns.>

Dependencies

- <name> <old> -> <new>. <What changed where it is used, and why behaviour
  is unchanged.>

Upgrade notes

None. No environment variables or routes changed.
```

House style:

- Write for someone deciding whether to upgrade. Explain why, not just what.
  "Bump dependencies" is not a release note: name the dependency, the
  versions and the reason.
- Say what did not change, for example "no configuration changed".
- `Upgrade notes` ends the message and is never left out. When there is
  nothing to do, say so explicitly.
- Commits that cancel out (a change and its revert) get a line saying so.
- Wrap at 79 columns.

## Never

- **Never hand-edit a version field to cut a release.** If you catch yourself
  editing one, the tag is what you wanted.
- **Never use a lightweight or unsigned tag.** Every release tag is signed,
  annotated and carries the notes.
- **Never tag a commit that is not on `origin/main`.** The workflow builds
  the tag from the remote.
- **Never move or force-push an existing tag.** Cut the next patch instead.
