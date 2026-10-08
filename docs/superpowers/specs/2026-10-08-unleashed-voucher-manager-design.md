# Unleashed Voucher Manager: design

Date: 2026-10-08

## Goal

Port [unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
(UVM) to Ruckus Unleashed, keeping as much of UVM's functionality as the
controller allows. This repository is a copy, not a fork: it was imported from
UVM at `87061684` without history and is maintained on its own from here.

The app runs on a read-write admin role. Unleashed's Guest Pass Manager role
can list and create passes but cannot delete them, which would rule out half
of UVM's features. The design goal is parity with UVM, not a subset.

## Verified against the live controller

Probed on 2026-10-08 against Unleashed `200.19`, logged in as a user whose
role has `admin-priv="rw"`. Every probe pass was deleted afterwards.

| Behaviour | Result |
|---|---|
| Login | `GET /admin/login.jsp?username=…&password=…&ok=Log In` answers `302` to `dashboard.jsp` on success, `200` on bad credentials. The CSRF token is in the `HTTP_X_CSRF_TOKEN` response header. |
| Session activation | The session is only usable after **following the redirect to `/admin/dashboard.jsp`**. Without that GET, every AJAX call answers `302` to the login page. aioruckus does not do this; this firmware needs it. |
| Session expiry | Any AJAX call answering `302` means the session is gone. |
| List | `POST /admin/_conf.jsp`, `text/xml`, `<ajax-request action='getconf' DECRYPT_X='true' updater='guest-list.<ms>' comp='guest-list'><guest self-service='!true'/></ajax-request>` |
| Create single | `POST /admin/mon_createguest.jsp` form, `gentype=single`. An empty `key` makes the controller generate one. Response is JSON with `result` `DONE`. |
| Custom key | `key=<value>` is stored uppercased. A key in use answers `result=KEY_DUPLICATED`. |
| Create batch | `gentype=multiple`, `createToNum` 2 to 100. Response has `batchEmailData.push('Guest-N|key|')` lines before the JSON, and `result` `OK`. Passes are named `Guest-N` by the controller (the submitted name is ignored) with remarks `Batch generation`. |
| Duration units | `hour`, `day`, `week` all honoured on this path. `min` and `minute` produce `valid-time=0`, a pass that expires at once. **Minutes are not supported.** |
| Share limit | `limitnumber=N` becomes `share-number`. `0` is unlimited devices. |
| Remarks | Free text, stored as `remarks`. |
| Delete | `POST /admin/_conf.jsp`, `<ajax-request action='delobj' updater='guest-list.<ms>' comp='guest-list'><guest id='N'></guest>…</ajax-request>`. Single and bulk both work. Delete is by `id` only. |
| Key generation endpoint | `mon_guestdata.jsp` returns an empty key for this account. Not used; send an empty key instead. |
| Expired passes | The controller removes them by itself, within seconds. |
| Names | A name containing whitespace silently creates nothing: the response is `result=OK` echoing the previous pass's name and key. Brackets, dots and dashes are fine (`[ROLLING]-125714-192.0.2.46` was accepted). Duplicate names are allowed. |
| Guest portal | A guest WLAN's guest service can set `redirect="url"` with `redirect-url` pointing at the app's `/welcome` page, which is what rolling vouchers need. The probed portal also had `countdown-by-issued="true"`. |
| `countdown-by-issued` | With it set on the portal, every pass has `start-time` equal to `create-time` and expires at `create-time + valid-time` whether used or not. Use is shown only by `used="true"` and nested `<client mac=…/>` elements. |

A listed pass looks like this (key and MAC sanitised):

```xml
<guest shared-guestpass="true" share-number="1" created-by="admin" role-id="2147483647"
  countdown-by-issued="true" create-time="1791475860" valid-time="7344000"
  start-time="1791475860" expire-time="1798819860" email="" phone-number=""
  reauth-interval-unit="min" remarks="" name="guest-1" x-key="000000" id="1"
  full-name="guest-1" wlan="Guest" used="true" key="000000">
  <client mac="00:00:00:00:00:01" />
</guest>
```

## Architecture

Unchanged from UVM: a Next.js frontend and an Axum backend in one container.
The frontend proxies `/rust-api/*` to the backend and applies
`GUEST_SUBNETWORK` gating. The backend holds the controller credentials.

`backend/src/unifi_api.rs` is replaced by `backend/src/unleashed_api.rs`,
which keeps the public method set the handlers already call:

| Method | Unleashed implementation |
|---|---|
| `try_new` | Build the HTTP client and log in |
| `get_all_vouchers`, `get_vouchers(filter)` | List, map, filter by name in the backend |
| `get_voucher_details(id)` | List, pick by id |
| `get_newest_voucher` | List, max by `create-time` |
| `get_rolling_voucher` | List, newest pass named `[ROLLING]-…` that is unused and unexpired |
| `create_voucher(request)` | Single or batch create, then list to return the created passes |
| `create_rolling_voucher()` | Under the create lock: return the current rolling voucher if one is waiting, otherwise create one |
| `delete_vouchers_by_ids(ids)` | One `delobj` with every existing id. Ids must be numeric; anything else is rejected with `400` before it reaches the XML. |
| `delete_expired_vouchers`, `delete_expired_rolling_vouchers` | List, filter expired, `delobj` |

Handlers, routes, tasks and the frontend change only where Unleashed forces it.

### Session

- `reqwest` with a cookie store. Certificate verification follows
  `UNLEASHED_HAS_VALID_CERT` (default `true`).
- Login: discover the login URL from the redirect on `GET /`, log in, take the
  CSRF token from the header (falling back to scraping `_csrfTokenVar.jsp`),
  then `GET dashboard.jsp`.
- The token goes on every AJAX request as `X-CSRF-Token`.
- One session is shared behind a lock. A `302` on any AJAX call triggers one
  serialised re-login and one retry; a second `302` is an authentication
  error.

### Data mapping

The JSON the frontend sees keeps UVM's `Voucher` shape, minus the fields
Unleashed cannot provide, plus `remarks`:

| `Voucher` field | Source |
|---|---|
| `id` | `id` |
| `createdAt` | `create-time`, formatted as UVM does in `TIMEZONE` |
| `name` | `full-name`, falling back to `name` |
| `code` | `x-key`, falling back to `key` |
| `authorizedGuestLimit` | `share-number`; `0` maps to `null` (unlimited) |
| `authorizedGuestCount` | number of `<client>` elements |
| `activatedAt` | `start-time` when `used="true"` or a client is present, else `null` |
| `expiresAt` | `expire-time` |
| `expired` | `expire-time <= now` |
| `timeLimitMinutes` | `valid-time / 60` |
| `remarks` | `remarks` (new) |
| `dataUsageLimitMBytes`, `rxRateLimitKbps`, `txRateLimitKbps` | Removed. Unleashed has no per-pass limits. |

`activatedAt` cannot use `start-time` alone, because with
`countdown-by-issued` the controller sets it at creation.

### Creating passes

`VouchersCreateRequest` keeps `count`, `name`, `authorizedGuestLimit` and
`timeLimitMinutes`, drops the data and rate fields, and adds optional `code`
and `remarks`.

- **Duration:** `timeLimitMinutes` is rounded up to whole hours, then sent in
  the largest unit that divides it exactly (`week`, then `day`, else `hour`).
- **Count 1:** `gentype=single` with `fullname=name`, `key=code` or empty.
- **Count 2 to 100:** `gentype=multiple`. The controller names the passes
  `Guest-N`, so the UI says the name is ignored for batches. `code` is
  rejected for batches.
- **Count over 100:** rejected with `400`.
- **Name and key validation** before sending: names have whitespace replaced
  with `-`; keys must be 2 to 16 characters with no whitespace,
  `# & + " ' < >` or comma.
- **Name and remarks characters**, probed on the controller: names cannot
  contain ``' " < > & # ; \ ` | ! $ ( )`` (it answers `OK` and creates
  nothing), remarks cannot contain `<` or `>` (it answers "Invalid Characters
  detected"), and both must be printable ASCII (it stores "é" as "Ã©"). The
  backend rejects these with `400` and the create forms check them first.
- **Errors:** `KEY_DUPLICATED` becomes `409` with the controller's message;
  an "Invalid Characters" result becomes `400`; other non-`DONE`/`OK` results
  become `502`.
- After creating, the backend lists and returns the passes that were not there
  before, matching UVM's `VouchersCreateResponse`. The response body alone is
  not trusted: if the list shows no new pass, the create failed and the
  backend answers `502`.

### Rolling vouchers

Same flow as UVM: the guest portal redirects to `/welcome`, which asks the
backend for the next rolling voucher. `/kiosk` shows the current one and
refreshes on server-sent events.

- One waiting voucher instead of UVM's one-voucher-per-IP rule: a rolling
  voucher is created only when no unused, unexpired one exists; otherwise the
  request returns the waiting one. Reloading `/welcome` therefore cannot mint
  more, and the kiosk recovers when its own voucher is used without
  `/welcome` loading, which a per-IP rule would refuse.

- Name: `[ROLLING]-<YYYYmmddHHMMSS>` (UVM's `[ROLLING] ` prefix had a space,
  which Unleashed rejects). UVM also put the guest's address in the name for
  its per-address rule; with one waiting voucher the address decides
  nothing, so it is left out.
- Current rolling voucher: newest `[ROLLING]-` pass that is unexpired and
  unused (by `used` and clients, not `start-time`).
- `ROLLING_VOUCHER_DURATION_MINUTES` (default `480`) is rounded up to hours.
- Share limit 1: a rolling voucher works on one device, so each guest gets
  their own and the voucher counts as used as soon as that device joins.
- For `GUEST_SUBNETWORK`, the guest's address is the last entry of
  `X-Forwarded-For`, the one the reverse proxy in front of the app appended;
  earlier entries come from the client and can be forged. The backend does
  not need the address.
- With `countdown-by-issued` on the portal, an unused rolling voucher expires
  and the controller removes it. The kiosk therefore also re-fetches every
  minute, without showing a spinner; when the fetch finds none, it creates
  one, as UVM's kiosk already does on load.

### Cleanup

`/api/vouchers/expired`, `/api/vouchers/expired/rolling`, the "delete expired"
button, the midnight purge and `PURGE_ALL_EXPIRED_VOUCHERS` are all kept for
parity. Because the controller removes expired passes itself, they will
usually find nothing to delete.

## Frontend changes

- **Custom Create:** duration units become hours, days and weeks. Data and
  rate limit inputs are removed. Optional key and remarks inputs are added.
  The name input notes that batches are named by the controller.
- **Voucher card and details:** drop the data and rate rows; show remarks.
- **Print:** drop `showDataUsageLimit`, `showRxRateLimit` and
  `showTxRateLimit` from `PRINT_CONFIG`; everything else stays.
- **Kiosk:** add the once-a-minute re-fetch.
- **Branding:** the RUCKUS dog on a white tile with a black border is the
  header logo (`public/logo.svg`), the centre of the WiFi QR code
  (`public/qr-logo.svg`) and the browser and home-screen icons. In dark mode
  the header logo is inverted with its hue rotated back (`invert` plus
  `hue-rotate-180`), giving a white dog on a black tile with the orange kept;
  the QR icon is not inverted, because the QR code turns white on dark and
  needs the white tile. Either SVG can be replaced by mounting a file.
- Everything else (Quick Create presets, browse and search, bulk select and
  delete, QR, theme, notifications, SSE, TestTab) is unchanged.

## Configuration

UVM's variables keep their names and defaults, except the controller
variables below and `IS_LOGO_INVERTIBLE`, which defaults to `true` for the
bundled logo and is parsed as a boolean (environment values arrive as
strings, and `"false"` is truthy):

| Removed | Added |
|---|---|
| `UNIFI_CONTROLLER_URL` | `UNLEASHED_URL` (required), for example `https://unleashed.example.com` |
| `UNIFI_API_KEY` | `UNLEASHED_USERNAME`, `UNLEASHED_PASSWORD` (required) |
| `UNIFI_HAS_VALID_CERT` | `UNLEASHED_HAS_VALID_CERT` (default `true`) |
| `UNIFI_SITE_ID` | `UNLEASHED_SSID` (required): the guest WLAN passes are created on |

The account needs a role with read-write admin privilege; the Guest Pass
Manager role cannot delete. The README explains how to create one.

## Build and release

- Hosted on Forgejo, push-mirrored to
  `github.com/greyrock-labs/unleashed-voucher-manager`.
- `.forgejo/workflows/ci.yaml` on pull requests and pushes to `main`: backend
  `cargo build --locked` and `cargo test --locked`; frontend `npm ci`,
  `tsc --noEmit`, `npm test`, `npm run build`. The runner image has Node 24
  and no Rust, so the workflows install Rust with `dtolnay/rust-toolchain`.
- `.forgejo/workflows/release.yaml` on `v*` tags: backend tests, then
  `docker-bake.hcl` target `image-all` (linux/amd64 only; the runner has no
  QEMU) pushed to `ghcr.io/greyrock-labs/unleashed-voucher-manager` with
  semver and `sha-` tags. The bake file sets the
  `org.opencontainers.image.source` annotation on index and manifest, pointing
  at the GitHub repo. Secrets: `GHCR_USERNAME`, `GHCR_TOKEN`.
- `.renovaterc.json5` extends the shared Renovate preset.
- The first release is `v1.0.0`.
- `.agents/skills/cutting-a-release/SKILL.md` documents releasing: commit to
  `main`, a signed annotated tag is the only version input, every
  code-changing commit gets a release, and the tag message house style.

## Testing

- **Unit tests** (backend):
  - guest-list XML parsing against fixtures from the probe, sanitised;
  - `Voucher` mapping, including `activatedAt` under `countdown-by-issued`
    and `0` share limit mapping to `null`;
  - `mon_createguest` response parsing: `DONE`, `OK` with the batch prefix
    lines, `KEY_DUPLICATED`;
  - minute rounding and unit choice;
  - count routing (1, 2 to 100, over 100) and name and key validation;
  - rolling voucher selection.
- **Mock-HTTP session tests:** login with the dashboard step, CSRF header on
  requests, re-login and single retry on `302`, failure on a second `302`.
- **Frontend:** typecheck and build, plus `node --test` unit tests
  (`src/**/*.test.ts`) for pure helpers such as boolean parsing.
- **Live check** for changes to how the app talks to the controller: run the
  container locally against the controller; create single, batch and
  custom-key passes, list, search, delete selected, print, and drive
  `/welcome` and `/kiosk`. Clean up everything created. Never test wrong
  credentials against a real controller: it then refuses logins from that
  address for a while.
- CI never talks to a controller.

## Robustness

- **Concurrent creates:** a lock is held from the guest list before a create
  to the guest list after it, so concurrent creates cannot claim each
  other's passes, and the rolling voucher check and create run as one step.
- **Credentials stay out of logs:** login sends the password in the query
  string, so request URLs never reach logs or error messages, and the
  configuration's `Debug` output redacts the password.
- **Startup:** connection attempts are retried after 5 seconds, doubling up
  to 5 minutes, and the log says whether the controller refused the
  credentials or could not be reached. Retrying slowly instead of exiting lets
  a corrected password or a returning controller recover without a restart.
- **Expiry:** a pass whose `expire-time` is empty or zero never counts as
  expired, so cleanup never deletes it.
- **Midnight purge:** where daylight saving skips midnight it runs at the
  first valid local time; where midnight repeats, at the first one.

## Deployment requirements

- The controller's guest portal must redirect to the app's `/welcome` page
  for rolling vouchers to roll.
- The reverse proxy directly in front of the app must append (or set) the
  client's address in `X-Forwarded-For`, and the app's port must not be
  reachable from the guest network directly. `GUEST_SUBNETWORK` gating
  depends on it.
- The app must be able to reach the controller over HTTPS.

## Out of scope

- Per-pass data and rate limits: Unleashed has none.
- Durations under one hour: the controller cannot express them.
- Ruckus One, SmartZone and ZoneDirector.
- The Guest Pass Manager role.
