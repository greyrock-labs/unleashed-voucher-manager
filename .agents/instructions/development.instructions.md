# Development

## Layout

- `backend/` is an Axum (Rust) server. It holds the controller credentials
  and is the only part that talks to the controller.
  - `src/unleashed/guest.rs`: parse the controller's guest list.
  - `src/unleashed/create.rs`: build guest pass create requests, validate
    names, keys and remarks, read create responses.
  - `src/unleashed/session.rs`: log in, keep the session and CSRF token,
    re-login once on a `302`.
  - `src/unleashed/mapping.rs`: turn guest passes into the `Voucher` JSON,
    rolling voucher rules.
  - `src/unleashed_api.rs`: the operations the HTTP handlers call. Creates
    hold a lock from the guest list before to the guest list after.
  - `src/handlers.rs`, `src/main.rs`, `src/tasks.rs`: routes, startup with
    backoff, the midnight purge.
- `frontend/` is Next.js. `src/proxy.ts` forwards `/rust-api/*` to the
  backend and applies `GUEST_SUBNETWORK`.
- `scripts/` run both processes in one container; `Dockerfile` and
  `docker-bake.hcl` build it.

## Commands

```bash
cd backend && cargo test --locked          # all backend tests
cd backend && cargo clippy --all-targets && cargo fmt --check
cd frontend && npm ci && npx tsc --noEmit && npm test && npm run build
docker buildx bake image-local             # image unleashed-voucher-manager:local
```

## Tests

- Write the failing test first, watch it fail, then implement.
- Backend tests live in `backend/tests/`, one file per module.
  `tests/support/mod.rs` is an in-process mock controller that reproduces
  observed controller behaviour (the dashboard step, CSRF checks, session
  expiry, silent no-create, `KEY_DUPLICATED`); extend it when the real
  controller turns out to behave differently.
- Fixtures in `backend/tests/fixtures/` are real controller responses with
  keys, MACs and WLAN names sanitised. Keep them sanitised.
- Frontend tests are `src/**/*.test.ts`, run by `node --test` with Node's
  built-in TypeScript support. The CI runner has Node 24, which needs the
  glob form in `npm test`; a bare directory argument only works on newer
  Node.
- CI never talks to a controller. For changes to how the app talks to the
  controller, also run the container against a real one (see
  `controller.instructions.md`).

## Running the app locally

The frontend works without a controller, which is enough to check the UI:

```bash
docker run --rm -d --name uvm-ui -p 3000:3000 \
  -e UNLEASHED_URL=https://192.0.2.1 -e UNLEASHED_USERNAME=x \
  -e UNLEASHED_PASSWORD=x -e UNLEASHED_SSID=Guest \
  unleashed-voucher-manager:local
```

The backend starts a few seconds after the container; wait for
`/rust-api/health` to answer before scripting requests. Headless Chrome
screenshots of the app never finish loading, because the page keeps a
server-sent events connection open; put a time limit on them, and use the
app's theme switcher in a real browser to check dark mode.

## Shell notes

- `sed` on this machine is GNU sed: use `sed -i`, not `sed -i ''`.
- `git cherry-pick` has no `-q` option.
