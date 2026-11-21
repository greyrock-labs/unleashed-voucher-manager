# AGENTS.md

## Purpose

`unleashed-voucher-manager` is a web app for Ruckus Unleashed guest passes:
create, browse, print and delete them, and hand out single-device rolling
vouchers on a kiosk page. It is a port of
[unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
(UVM), maintained on its own. The design and the verified controller
behaviour are in `docs/superpowers/specs/2026-11-20-unleashed-voucher-manager-design.md`;
read it before changing how the app talks to the controller.

Image: `ghcr.io/greyrock-labs/unleashed-voucher-manager` (`linux/amd64`,
`linux/arm64`). GitHub mirror: `greyrock-labs/unleashed-voucher-manager`.

## Layout

```text
backend/                     Axum (Rust); the only part that talks to the controller
  src/unleashed/guest.rs     parse the guest list
  src/unleashed/create.rs    build create requests; name, key and remarks rules; read responses
  src/unleashed/session.rs   login, dashboard step, CSRF token, one shared re-login on 302
  src/unleashed/mapping.rs   guest pass -> Voucher JSON; rolling voucher rules
  src/unleashed_api.rs       operations the handlers call; the create lock
  src/startup.rs             connection backoff and messages
  src/tasks.rs               midnight purge
  tests/                     one file per module; support/mod.rs is the mock controller
  tests/fixtures/            real controller responses, sanitised
frontend/                    Next.js; src/proxy.ts forwards /rust-api/* and applies GUEST_SUBNETWORK
scripts/                     entrypoint, process wrapper and healthcheck for the container
Dockerfile                   amd64 and arm64 without emulation (cargo-zigbuild, sharp swap)
```

## Tooling

mise pins Rust, Node, lefthook and oxfmt and holds the tasks (`mise tasks`):
`mise run test`, `lint`, `fmt-check`, `oxfmt-check`, `frontend-build`,
`docker-build`. lefthook runs the format checks before commits and the tests
before pushes. Build the image with `mise run docker-build`: the Dockerfile has no
version defaults, so the Rust and Node versions always come from mise. oxfmt formats YAML, JSON and Markdown only; `backend/` and
`frontend/src/` keep UVM's formatting so later UVM changes port cleanly.

## Testing

- Write the failing test first and watch it fail.
- The mock controller reproduces observed behaviour: the dashboard step,
  CSRF checks, session expiry, names with whitespace that create nothing,
  `KEY_DUPLICATED`, invalid characters, `Guest-N` batch names. Extend it when
  the real controller turns out to behave differently.
- Keep fixtures sanitised: no real keys, MACs, WLAN names or pass names.
- Frontend tests are `src/**/*.test.ts`, run by `node --test`; keep the
  quoted glob in `npm test`.
- CI never talks to a controller.

The frontend runs without a controller, which is enough to check the UI:
start the image with dummy `UNLEASHED_*` values. The backend only listens
once it reaches a controller, so the container is not healthy then.

## Live testing against a controller

The controller serves real guests, so:

- **Never send wrong credentials.** A few failed logins make the controller
  refuse logins from that address for a while, even with the right password,
  with no event or alarm. Unit tests cover that path.
- Ask before writing. Keep any copy of the credentials outside the
  repository with owner-only permissions and delete it afterwards.
- Record the existing passes first, create only passes with test-like names
  (`probe-<time>`), delete only the ids you created, and confirm the list
  matches afterwards. Check names via `full-name`: some firmware lists no
  `name` attribute.
- A rolling voucher waiting on a kiosk belongs to the deployment: do not use
  it up or delete it. With one waiting, a rolling request returns it without
  creating anything, a safe check of the rolling path.
- Raw probing: `GET /admin/login.jsp?username=...&password=...&ok=Log In`,
  take the `HTTP_X_CSRF_TOKEN` header, load `dashboard.jsp`, then post to
  `/admin/_conf.jsp` (`text/xml`) or `/admin/mon_createguest.jsp` (form) with
  `X-CSRF-Token`.

## Workflow and releases

- Commit to `main` as focused, signed, Conventional Commits; push low-risk
  changes without asking, and ask before anything that publishes or could
  break a deployment.
- Releases go through a release pull request, never through hand-made tags.
  Never create, push or move a `v*` tag by hand.

### Release process

- `.forgejo/workflows/release-pr.yaml` ("Prepare release") runs on every push
  to `main`. If releasable commits have landed since the last `v*` tag, the
  `keymaker` bot force-pushes `release/next` with an updated `CHANGELOG.md` and
  keeps one PR open, titled `chore(release): vX.Y.Z`, with the release notes as
  its body. An open release PR stays open and is refreshed on every push; only
  a person closes it. Running the workflow by hand (`workflow_dispatch`) opens
  one for at least a patch even when nothing releasable is pending.
- Merging that PR is the decision to release, and the maintainer makes it: do
  not merge a release PR unless explicitly told to.
  `.forgejo/workflows/release.yaml` ("Publish release") runs only when
  `CHANGELOG.md` changes on `main`, or by hand to retry a failed release. It
  sees an untagged version at the top of `CHANGELOG.md`, runs the tests, builds
  `linux/amd64,linux/arm64`, pushes exactly `X.Y.Z` and `latest` to
  `ghcr.io/greyrock-labs/unleashed-voucher-manager`, signs the digest with
  cosign, tags the commit `vX.Y.Z` through the Forgejo API, and publishes the
  notes as a Forgejo release and a GitHub release on the mirror. Every step can
  be re-run safely.
- Pushes to `main` publish no images. There are no `:main`, `:sha-*` or `X.Y`
  tags; ask before adding any tag that moves.
- The Test workflow's image build keeps a BuildKit cache in the release bot's
  container registry on Forgejo, as `unleashed-voucher-manager-cache:amd64`.
  It is a cache, not an image: never pull or deploy it. Runs without the
  secret build without it, and a failed cache upload never fails the build.

Versions come from conventional commits (git-cliff, `cliff.toml`; before the
first tag, the first release is its `initial_tag`, `v1.0.0`):

| Commit                                                             | Release |
| ------------------------------------------------------------------ | ------- |
| breaking change (`feat!:` or `BREAKING CHANGE:`)                   | major   |
| `feat:`                                                            | minor   |
| `fix:`, `perf:`, `fix(deps):`                                      | patch   |
| `chore:`, `ci:`, `docs:`, `test:`, `build:`, `refactor:`, `style:` | none    |

Renovate extends the shared preset but overrides its commit types in
`.renovaterc.json5`. Frontend runtime dependencies, backend crates and the
Rust and Node toolchains are `fix(deps)`, which releases a patch. Everything
else is `chore` and never carries `!`.

The scripts behind this live in `.forgejo/scripts/`, and
`mise run release-scripts-test` tests them. Repository secrets:
`RELEASE_BOT_TOKEN` (the `keymaker` Forgejo token, with repository and issue
write, `read:user` for its own name and email, and package write for the build
cache), `GHCR_TOKEN` (a GitHub
classic PAT with `write:packages` and `public_repo`, used for image pushes and
GitHub releases), and `COSIGN_PRIVATE_KEY` plus `COSIGN_PASSWORD` (the signing
key; the public half is `cosign.pub`). Images carry an SBOM but no provenance
attestation.

- Renovate updates dependencies; an npm update must change
  `package-lock.json` in the same commit, or `npm ci` fails in CI.

## Documentation rules

- Each change updates the README, the spec or this file where it affects
  them, ideally in the same commit.
- Describe this repository only: no internal hostnames, addresses, SSIDs,
  usernames or credential store names, and no history of earlier attempts.
  Use `example.com` and `192.0.2.x` in examples.
- Grep every diff and commit message for internal details before pushing;
  amending afterwards does not remove a pushed commit from the remotes.
