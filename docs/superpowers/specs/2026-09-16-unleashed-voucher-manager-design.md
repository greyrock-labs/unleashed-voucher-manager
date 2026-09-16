# Unleashed Voucher Manager — Design

Date: 2026-09-16
Status: Approved, ready for implementation planning

## Goal

Replace `unifi-voucher-manager` (UVM) in the k8s cluster with an equivalent
that talks to a Ruckus Unleashed controller instead of a UniFi one.

The primary job is narrow: **a guest pass is created once a day, good for
24 hours, and displayed on a guest-facing page alongside the WiFi QR code.**
Secondary: a small admin UI to create ad-hoc passes and browse existing ones.

## Sources

| Project | License | Role |
|---|---|---|
| [etiennecollin/unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager) | MIT | Base tree: frontend, backend structure |
| [fmuffat/FetchPass](https://github.com/fmuffat/FetchPass) | MIT | Reference for the Unleashed guest-pass protocol |
| [ms264556/aioruckus](https://github.com/ms264556/aioruckus) | 0BSD | Reference for headless session handling |

FetchPass drives headless Chrome so that its XHRs inherit the browser's
session cookie and CSRF token. That is a convenience, not a requirement:
this project performs the same login over plain HTTPS. No browser ships in
the container.

## Verified against the live controller

Firmware `200.19.7.112.238`, account `guestpass` (Guest Pass Manager role).
Every row below was confirmed empirically on 2026-09-16, not inferred.

| Behaviour | Result |
|---|---|
| Headless login + CSRF token extraction | works, no browser |
| `getstat` `system` `<guest-list/>` | works |
| `docmd` `generate-guest-key` | works |
| `docmd` `create-guest` | works |
| `docmd` `delete-guest` (nested and attr forms) | `AD_PrivilegeInsufficient` |
| `delobj` comp `guest-list` (both endpoints) | `AD_PrivilegeInsufficient` |
| `getconf` anything | `AD_PrivilegeInsufficient` |
| `duration-unit='day'` | silently treated as hours — `1 day` yields a 1-hour pass |
| `countdown-by-issued='true'` | silently ignored, returns `false` |
| `expire-time` / `valid-time` create attributes | silently ignored |
| `shared='false'` | silently ignored, always `true` |
| Duplicate pass name | rejected, `E_DuplicatedValue`, nothing created |
| Name charset | 1–64 chars; no whitespace, and none of &#33; &#35; &#36; &#38; &#40; &#41; &#60; &#62; &#34; &#39; &#92; &#124; &#59; &#96; &#44; |
| `share-number='0'` | unlimited devices; what the admin UI sends for "unlimited" |
| Unused-pass claim window | 7 days, fixed; identical for admin-UI and API creates |

### Two distinct time fields

This tripped up both the initial design and a reading of the admin UI, so it
is recorded explicitly:

- **`valid-time`** — seconds of access granted *once the pass is first used*.
  This is what `duration` sets. A 24-hour pass has `valid-time=86400`.
- **`expire-time`** — for an unused pass, the deadline by which it must be
  *first used* (create-time + 7 days, not configurable). On activation the
  controller recomputes it to `start-time + valid-time`.

The admin UI's "expiry" column shows `expire-time`, so an unused 1-hour pass
displays a date 7 days out. That is correct behaviour, not a misconfiguration.

### Consequence: no deletion

The Guest Pass Manager role cannot delete. Its own UI has no delete control
(`guestinfo.js` contains no delete code at all). Therefore:

- Old daily passes accumulate in the list and cannot be pruned by this app.
- An unused code stays claimable for its full 7-day window, so a new daily
  code does **not** invalidate yesterday's.
- Whether the controller self-purges expired passes is **unknown**; the
  pass `t162903-dur24hr` reaches its deadline on 2026-09-23 and will answer
  it. Nothing in the build depends on the answer.

Accepted for v1. The only remedy is admin credentials, which can be added
later as a second endpoint surface without reworking the client.

## Scope

**In:** daily pass creation and display; ad-hoc create (name, duration,
share count); browse/search; WiFi QR; custom logo; dark/light; Helm chart;
Forgejo CI/release.

**Out:** printing (`/print`, `PRINT_CONFIG`); UVM's rolling vouchers
(`/welcome`, IP tracking, captive-portal redirect); per-voucher data and
rate limits; site IDs; **all deletion** — bulk delete, expired cleanup,
retiring yesterday's pass.

## Architecture

Unchanged from UVM: Next.js frontend, Axum backend, one container, frontend
proxies to the backend so credentials never reach the browser.
`backend/src/unleashed_api.rs` replaces `unifi_api.rs`; everything else is
trimming.

### Unleashed client

    login()   GET  /user/user_login_guestpass.jsp      prime -ejs-session- cookie
              POST username, password, ok="Log in"
              scrape  var csfrToken = '...'
    call()    POST /user/_cmdstat.jsp
              Content-Type: application/x-www-form-urlencoded
              X-CSRF-Token: <token>   Referer: /user/guestinfo.jsp
              <ajax-request action=.. updater=<comp>.<ms>.<rand> comp=..>..</ajax-request>
    list()    getstat system <guest-list/>
    create()  docmd generate-guest-key -> x-key, then docmd create-guest

`reqwest` with a cookie store. Certificates verified normally (the
controller runs a Let's Encrypt cert); `UNLEASHED_HAS_VALID_CERT=false`
disables verification for self-signed deployments.

Session state (cookie jar + CSRF token) sits behind a lock. A response whose
body begins `<!DOCTYPE html>` means the session lapsed: re-login once and
retry the call, failing the request if the retry also lapses.

**The client has no delete method.** Not stubbed, not feature-gated — absent,
so the impossibility is expressed in the type system rather than in comments.

Two invariants enforced in the client, both derived from verified behaviour:

- Names are sanitised to the controller's charset and length before send.
- Durations are always converted to whole hours and sent as
  `duration-unit='hour'`, matching both FetchPass and the controller UI's
  own `changeDurationToHours`.

### Model

    GuestPass {
      id, name, code,            // code <- x-key
      ssid, created_at,
      activated_at: Option,      // <- start-time, None when unused
      expires_at, valid_time_secs,
      used: bool, share_number,  // 0 = unlimited
      client_macs: Vec<String>,  // <- nested <client mac=..>
      remarks,
    }

`expired` and `active` are derived, never stored.

### API

    GET  /api/health          includes daily-pass freshness
    GET  /api/passes          list, optional name filter
    POST /api/passes          create { name, duration_hours, share_number }
    GET  /api/passes/daily    today's code

All of UVM's delete routes are removed.

### Daily rotation

Replaces UVM's `run_daily_purge`. It only creates; it never deletes.

On startup and once daily at `DAILY_ROLL_HOUR` (`TIMEZONE`-aware):

1. Determine the current period — the window that began at the most recent
   `DAILY_ROLL_HOUR`, **not** the calendar date. At 02:00 with a 04:00 roll
   hour, the active period still began yesterday, so there is no window in
   which today's pass does not yet exist.
2. Attempt `create-guest` named `daily-YYYY-MM-DD` for that period start,
   with `DAILY_DURATION_HOURS` (default 24) and `DAILY_SHARE_NUMBER`
   (default `0`, unlimited).
3. Treat `E_DuplicatedValue` as success. The controller's name-uniqueness
   check is the idempotency mechanism — no read-before-write, no race, and
   two replicas cannot both mint a code.

`share-number` defaulting to `0` is load-bearing: at `1` the daily code
would admit exactly one device and silently lock out every other guest.

Lookup resolves today's code by the expected name, falling back to the most
recently created `daily-`-prefixed pass if that name is missing (task was
down, clock drift, failed create). The display then shows the last good code
with its true expiry, and `/api/health` reports it stale rather than the page
going blank.

### Frontend

**Keep:** Quick Create, Custom Create (name, duration, share count only),
list/search, WiFi QR, custom logo, dark/light, notifications, SSE.

**Remove:** `print/`, `welcome/`, `kiosk/`, `TestTab`, `PRINT_CONFIG`, the
data/rate-limit fields, all bulk-select and delete UI.

**Add:** `/display` — read-only, today's code set large beside the WiFi QR.

### Configuration

`UNLEASHED_URL`, `UNLEASHED_USERNAME`, `UNLEASHED_PASSWORD`,
`UNLEASHED_SSID`, `UNLEASHED_HAS_VALID_CERT` (default true), `TIMEZONE`,
`DAILY_ROLL_HOUR` (default 4), `DAILY_DURATION_HOURS` (default 24),
`DAILY_SHARE_NUMBER` (default 0), the `WIFI_*` set, `IS_LOGO_INVERTIBLE`,
bind hosts/ports, `BACKEND_LOG_LEVEL`.

Removed: all `UNIFI_*`, `GUEST_SUBNETWORK`, `PURGE_ALL_EXPIRED_VOUCHERS`,
`ROLLING_VOUCHER_DURATION_MINUTES`, `PRINT_CONFIG`.

### Build and deployment

`.forgejo/workflows/{ci,release}.yaml` follow the pattern established in
`cert-manager-webhook-cloudns`: pinned action SHAs, `runs-on: docker`, GHCR
login, `docker-bake.hcl` with `image-all` at `linux/amd64` only (the Forgejo
runner cannot mount binfmt_misc, so no QEMU), metadata-action tags, and an
OCI Helm push to `ghcr.io/greyrock-labs/helm`. Rust and Node toolchains
replace Go. New chart at `deploy/unleashed-voucher-manager`.

### Testing

Fixtures are real responses captured from the controller on 2026-09-16:
a populated `<guest-list/>` including nested `<client>`, a successful
`create-guest`, and the `AD_PrivilegeInsufficient`, `E_FailGuestName` and
`E_DuplicatedValue` error envelopes.

Covered: XML parsing, name sanitisation, hour conversion, period-start
naming across the roll boundary, `E_DuplicatedValue` treated as success,
stale-pass fallback, and login/re-login against a mock HTTP server.

No live-controller calls in CI.

## Risks

1. **Old codes stay claimable for 7 days.** Inherent to the role; accepted.
2. **Unbounded list growth** if the controller does not self-purge. Answer
   due 2026-09-23. Worst case is manual pruning.
3. **Login page scraping.** The CSRF token is read from inline JS, so a
   firmware upgrade could change its shape. Contained to one function, with
   a clear error rather than a silent failure.
4. **Session handling under concurrency** is the most delicate part of the
   client and warrants the most test attention.
