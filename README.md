# Unleashed Voucher Manager

A small self-hosted web app that mints a guest WiFi code every day on a
**Ruckus Unleashed** controller and displays it, next to a scannable WiFi QR
code, on a wall-mounted tablet or kiosk. It also has a lightweight admin UI
for creating one-off guest passes.

It is not affiliated with, endorsed by, or sponsored by CommScope, RUCKUS
Networks, or Ubiquiti Inc. See [NOTICE](./NOTICE) for full attribution —
this project is derived from
[etiennecollin/unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager),
with the Unleashed protocol work informed by
[fmuffat/FetchPass](https://github.com/fmuffat/FetchPass) and
[ms264556/aioruckus](https://github.com/ms264556/aioruckus).

## Read this before you file a bug

Three behaviours below come from how the Unleashed controller and its
"Guest Pass Manager" account role work, not from bugs in this app. All
three were verified empirically against a live controller. The second one
is fixed by configuring the controller correctly; the other two are not
fixable at all.

### 1. This app cannot delete passes

The account it authenticates as (a Guest Pass Manager, deliberately scoped
down from a full admin) is refused by the controller on every delete
attempt — every delete-shaped request returns `AD_PrivilegeInsufficient`.
The controller's own guest-management UI has no delete button for this role
either. This is not a missing feature; there is no code path that could add
it without a second, full-admin credential.

**Consequence:** old daily passes accumulate in the guest list forever. You
must periodically delete expired/unwanted passes yourself in the Unleashed
admin UI (`Users -> Guest Access -> Guest Pass List`, or similar, depending
on firmware). This app cannot do it for you.

### 2. Pass lifetime hangs on a WLAN setting you must set yourself

The guest WLAN decides when a pass's validity clock starts — it is a
property of the network, not a field this app could send per pass. **Set
it to "Effective from the creation time"** on the SSID you point
`UNLEASHED_SSID` at; see [Controller setup](#controller-setup) below.
Everything here assumes it.

Left on the other option, "Effective from first use", a pass's duration
only begins counting when a guest first connects, and until then the code
stays claimable for a separate unused window (7 days by default). The
visible symptom is that **a new daily code does not invalidate yesterday's**
— a guest who copied one off the wall last week can still claim it.

### 3. `DAILY_SHARE_NUMBER=0` (the default) means unlimited devices

`DAILY_SHARE_NUMBER` controls how many distinct devices may use the daily
code. `0` is unlimited and is the default — appropriate for a shared
guest code. **Setting it to `1` admits exactly one device and locks out
every other guest** who tries the same code afterwards. Only change this
if you specifically want a single-use-per-device code.

## What it does

- **Daily rotation.** Once a day, at `DAILY_ROLL_HOUR` (in `TIMEZONE`), the
  backend mints a new guest pass named `daily-YYYY-MM-DD`, good for
  `DAILY_DURATION_HOURS` from the moment it is minted, shared by
  `DAILY_SHARE_NUMBER` devices (default: unlimited). It also mints one on
  startup if today's pass doesn't exist yet.
- **`/display`** — a read-only, guest-facing page: today's code in large
  text and a WiFi QR code beside it (if `WIFI_SSID`/`WIFI_PASSWORD` are
  configured). Deliberately nothing else — it is meant to be left open on a
  tablet or TV near your guest network and read from across a room.
- **`/`** — the admin UI: Quick Create (preset durations, one click),
  Custom Create (name, duration, device-share count), and a browsable/
  searchable list of existing passes. No delete controls exist here either,
  for the reason above.
- **Custom SVG logo**, dark/light theme, live updates over SSE so `/display`
  and `/` pick up new passes without a manual refresh.

### How the "current period" is determined

The daily rotation doesn't key off the calendar date directly — it keys off
the most recent `DAILY_ROLL_HOUR`. Example: with the default roll hour of
`4` (4am), at 2am the "current period" is still the one that began
yesterday at 4am, so there's never a window between midnight and the roll
hour where today's pass doesn't exist yet but "today's" name is expected.
The pass is always named `daily-<period-start-date>`.

If a roll is ever missed (backend was down, clock drift, controller was
briefly unreachable), `/display` and `/api/passes/daily` fall back to
showing the most recently created `daily-*` pass rather than going blank,
and `/api/health` reports it as stale (`dailyPassCurrent: false`).

### `valid-time` vs. `expire-time` — read this carefully

This distinction has confused people working on this project before, so it
is spelled out explicitly:

- **`validTimeSecs`** is how much access time a pass grants. This is what
  `DAILY_DURATION_HOURS` / the "Duration" field sets.
- **`expiresAt`** is when access ends. With the controller configured as
  [Controller setup](#controller-setup) requires, that is simply
  `createdAt + validTimeSecs`.

`activatedAt` — and the Used/Available badge in the admin UI — records when
a guest first connected. It no longer has any bearing on when the pass
dies; it is there to tell you whether anyone has claimed the code yet.

Against a controller left on **first-use** validity, `expiresAt` instead
means two different things: for an unused pass it is the deadline to first
use it (creation plus the "expire if not used" window, a week by default),
and only for a used pass is it the real end of access. The UI labels it
"Expires" either way, so an unclaimed 1-hour pass reads as expiring a week
out. Setting the controller correctly is what keeps that label honest.

### Durations are always whole hours

On the `create-guest` API the `duration` value is interpreted in hours, and
`duration-unit` does not scale it: sending `duration='1' duration-unit='day'`
yields a **1-hour** pass, not a 1-day one. The Unleashed admin UI gets this
right because it converts to hours before sending (its own bundle calls
`changeDurationToHours` on the way out), so picking "1 day" there transmits
`duration=24`.

This app takes the same approach for the same reason: it only ever sends
`duration-unit='hour'` and expresses every duration — including
`DAILY_DURATION_HOURS` and the Custom Create form — in whole hours.

## Controller setup

Some of what this app does depends on how the **guest WLAN itself** is
configured, not on anything the app can send. These settings live on the
Wi-Fi network, so they must be set on the SSID you name in
`UNLEASHED_SSID`, and they apply to no other network.

Reach them at **Wi-Fi -> Wi-Fi Networks -> Wi-Fi Networks List**, pick the
guest network, then **Edit Wi-Fi Network -> Guest Details**.

### Required: Effective Date of Validity Period

> **"Effective from the creation time"**

This is the one that matters most. It starts a pass's clock the moment the
pass is minted, which is what the daily rotation assumes:

- the daily code is good for exactly `DAILY_DURATION_HOURS` from the roll;
- yesterday's code stops working on schedule instead of lingering;
- `expiresAt`, in the API and in the UI, always means when access ends.

The other option — "Effective from first use, expire if not used *N* days"
— starts the clock on first connection instead, and is what constraint #2
above describes. The app still runs against it, but the daily rotation
stops being a rotation: every past code stays claimable for the full unused
window.

**Changing this is retroactive**, verified on a live controller: passes
minted before the switch are re-evaluated under the new setting rather than
keeping the semantics they were created under. Flipping it does not leave
you with a tail of old codes behaving the old way.

The radio governs *when* the clock starts, never *how long* it runs. The
controller's own help text on this field says so — "Validity duration can
be configured when creating guest pass" — and that is exactly what this app
does: every `create-guest` carries an explicit `duration`.

### Required: Guest Password

> **"Unique password for each guest"**

The whole app is built on minting a distinct code per pass. "Single shared
password among all guest" hands every guest the same static password
instead, leaving nothing for the daily rotation to rotate.

### Required: Guest Authentication

Must be a mode that issues guest passes — "Guest Pass and Social Login" is
the configuration this was verified against. A mode without guest passes
gives `create-guest` nothing to create.

### Not required: Guest Friendly Key

On in the verified configuration, where it yields ten-character codes. The
admin UI and `/display` hyphenate those 5-5 for legibility; any other
length renders unsplit, so turning this off costs readability and nothing
else.

### Unrelated: Grace Period

The `480 minutes` field on the same screen is the Grace Period, which
governs reconnection, not pass validity. Nothing here reads or depends on
it.

### The tradeoff creation-time validity buys

A pass expires `DAILY_DURATION_HOURS` after it is created, whatever time a
guest turns up. With the defaults — roll at 4am, 24-hour duration — someone
connecting at 3am gets an hour of access, not a day.

If that matters, set `DAILY_DURATION_HOURS` above `24` so consecutive days
overlap. At `30`, each code outlives the next roll by six hours: nobody
ever gets less than six hours, at the cost of yesterday's code staying live
until 10am.

## Quick start (Docker Compose)

1. Copy [`compose.yaml`](./compose.yaml) and fill in your controller details:

   ```yaml
   services:
     unleashed-voucher-manager:
       image: "ghcr.io/greyrock-labs/unleashed-voucher-manager:latest"
       container_name: "unleashed-voucher-manager"
       restart: "unless-stopped"
       ports:
         - "3000:3000"
       environment:
         UNLEASHED_URL: "https://unleashed.example.com"
         UNLEASHED_USERNAME: "guestpass"
         UNLEASHED_PASSWORD: "changeme"
         UNLEASHED_SSID: "Guest"
         UNLEASHED_HAS_VALID_CERT: "true"
         TIMEZONE: "UTC"
         DAILY_ROLL_HOUR: "4"
         DAILY_DURATION_HOURS: "24"
         DAILY_SHARE_NUMBER: "0"
         WIFI_SSID: "Guest"
         WIFI_PASSWORD: ""
   ```

2. Start it:

   ```bash
   docker compose up -d
   ```

3. Admin UI: `http://localhost:3000/`. Guest display: `http://localhost:3000/display`.

`UNLEASHED_USERNAME`/`UNLEASHED_PASSWORD` should be a Guest Pass Manager
account on the controller, not a full admin — it doesn't need admin rights
for anything this app does, and per constraint #1 above, it *can't* delete
even if you wanted it to.

### Without Docker

Requires `rust >= 1.88`, `nodejs >= 24.3`, `npm >= 11.4`.

```bash
# Backend
cd backend && cargo run --release
# ...or with a .env file:
cd backend && cargo run --release --features dotenv

# Frontend (separate terminal)
cd frontend && npm install && npm run dev   # development
cd frontend && npm ci && npm run build && npm run start  # production
```

### Kubernetes / Helm

A chart lives at [`deploy/unleashed-voucher-manager`](./deploy/unleashed-voucher-manager),
published to `oci://ghcr.io/greyrock-labs/helm`. Non-secret settings go
under `config:` in `values.yaml` (same keys as the environment variable
table below); `UNLEASHED_USERNAME`/`UNLEASHED_PASSWORD` are supplied via
`existingSecret`, the name of a Secret you create yourself with those two
keys. `WIFI_PASSWORD` is secret-shaped too — either set it as a plain
`config.WIFI_PASSWORD` value or fold it into the same `existingSecret`,
whichever fits how the rest of your cluster handles credentials.

Keys left empty under `config:` are **not rendered into the Deployment at
all**, rather than rendered as `value: ""`. Kubernetes gives `env`
precedence over `envFrom`, so anything the chart renders would shadow the
`existingSecret` instead of deferring to it; and the app distinguishes
"variable unset" from "variable set to the empty string", so an empty
`WIFI_TYPE` is an *invalid* type rather than "use the default" and would
disable the QR code. If your guest network is genuinely open — no password
at all — set `wifiOpenNetwork: true`, which is the one supported way to
render a deliberately empty `WIFI_PASSWORD`. It is mutually exclusive with
`config.WIFI_PASSWORD`; setting both fails the render.

```bash
helm install guest-wifi oci://ghcr.io/greyrock-labs/helm/unleashed-voucher-manager \
  --set config.UNLEASHED_URL="https://unleashed.example.com" \
  --set config.UNLEASHED_SSID="Guest" \
  --set config.WIFI_SSID="Guest" \
  --set existingSecret="unleashed-voucher-manager-credentials"
```

**Object naming.** Objects are named `<release>-<chart>`, except that the
release name is not prefixed when it already contains the chart name. A Flux
`HelmRelease` named `unleashed-voucher-manager` therefore produces a Service
called `unleashed-voucher-manager`, not the name doubled. `nameOverride` and
`fullnameOverride` work as in any standard chart.

**Restarting on credential rotation.** If `existingSecret` is maintained by
something that rotates it (an ExternalSecret, say), the pod keeps using the
credential it started with until it restarts. The app does not crash on a
stale credential — it fails authentication against the controller, which is
quieter and easier to miss. Put whatever your cluster uses to trigger a
restart under `podAnnotations`, for example
`reloader.stakater.com/auto: "true"` for Stakater Reloader.

**Security context.** `podSecurityContext` and `securityContext` are passed
through untouched. The image already runs as `appuser` (uid/gid 1001), so a
non-root baseline needs no special handling.

Running with a read-only root filesystem needs `readOnlyRootFilesystem.enabled:
true` rather than `securityContext.readOnlyRootFilesystem` — the chart fails
the render if you set the latter, because the flag also mounts the three paths
the app writes to. A read-only root on its own would break it at startup:

| Path | Why |
| --- | --- |
| `/app/frontend/public` | the entrypoint writes `runtime-config.json` on every start |
| `/app/frontend/.next/cache` | Next.js image and fetch cache |
| `/tmp` | general scratch |

`/app/frontend/public` also holds the logo and favicon baked in at build time,
so an empty volume mounted there would hide them. The chart runs an init
container from the same image to copy those assets into the volume first.

## Custom SVG logo

- Docker: mount your SVG at `/app/frontend/public/logo.svg` (see the
  commented-out volume in `compose.yaml`). The mount path, including the
  filename, cannot be changed.
- Without Docker: place it at `frontend/public/logo.svg`.
- Set `IS_LOGO_INVERTIBLE=true` if your logo should be color-inverted in
  dark mode (e.g. a dark logo on a transparent background).

## Configuration

Backend-required variables are marked accordingly; everything else has a
working default.

| Variable | Default | Description |
|---|---|---|
| `UNLEASHED_URL` | — (**required**) | Base URL of the Unleashed controller's web UI, with scheme. Example: `https://unleashed.example.com` or `https://192.168.1.1:9080`. |
| `UNLEASHED_USERNAME` | — (**required**) | Login for a Guest Pass Manager account on the controller. Does not need admin rights (and admin rights wouldn't grant delete either — see constraint #1). |
| `UNLEASHED_PASSWORD` | — (**required**) | Password for that account. |
| `UNLEASHED_SSID` | — (**required**) | The guest SSID name to associate created passes with. |
| `UNLEASHED_HAS_VALID_CERT` | `true` | Set to `false` if the controller uses a self-signed certificate (common when connecting directly to its IP instead of through a reverse proxy). Getting this wrong will prevent all controller communication. |
| `TIMEZONE` | `UTC` | [IANA timezone identifier](https://en.wikipedia.org/wiki/List_of_tz_database_time_zones), e.g. `America/New_York`. Governs `DAILY_ROLL_HOUR` and all displayed times. |
| `DAILY_ROLL_HOUR` | `4` | Local hour (0–23) at which the next daily pass is minted. See "How the current period is determined" above. |
| `DAILY_DURATION_HOURS` | `24` | Whole hours of access the daily pass grants once activated. Minimum 1. |
| `DAILY_DURATION_HOURS` | `24` | Whole hours of access the daily pass grants, counted from the moment it is minted. Minimum 1. Assumes the controller is set to creation-time validity — see [Controller setup](#controller-setup). |
| `WIFI_SSID` | unset | SSID encoded into the WiFi QR code on `/display`. Required (with `WIFI_PASSWORD`) for the QR to render. |
| `WIFI_PASSWORD` | unset | Password encoded into the WiFi QR code. Use an empty string `""` for an open network with no password — **omitting the variable entirely, rather than setting it to `""`, is what silently disables the QR code** (the frontend needs to know a password was deliberately left blank vs. never configured). |
| `WIFI_TYPE` | `WPA` if password set, else `nopass` | `WPA`, `WEP`, or `nopass`. |
| `WIFI_HIDDEN` | `false` | Whether the SSID is broadcast or hidden. |
| `IS_LOGO_INVERTIBLE` | `false` | Whether the custom logo should be inverted in dark mode. |
| `BACKEND_BIND_HOST` | `127.0.0.1` | Address the Rust backend binds to. Only matters if you're not using the bundled Docker image. |
| `BACKEND_BIND_PORT` | `8080` | Port the Rust backend binds to. |
| `BACKEND_LOG_LEVEL` | `info` | `trace`\|`debug`\|`info`\|`warn`\|`error`. |
| `FRONTEND_BIND_HOST` | `0.0.0.0` (in the image) | Address the Next.js frontend binds to. |
| `FRONTEND_BIND_PORT` | `3000` (in the image) | Port the Next.js frontend binds to; this is the port you expose/publish. |
| `FRONTEND_TO_BACKEND_URL` | `http://127.0.0.1` | How the frontend reaches the backend internally. Only relevant if you split frontend and backend across hosts/containers, which the bundled image does not do. |

> [!IMPORTANT]
> `WIFI_SSID` and `WIFI_PASSWORD` must both be set for the QR code to
> appear. If either is missing, the QR code silently does not render —
> `/display` still shows the code and expiry text, just no QR — and the
> only trace is a `console.warn` in the browser devtools. There is no
> visible error on the page itself, so if your QR code isn't showing up,
> check these two variables first.

## API

| Route | Method | Notes |
|---|---|---|
| `/api/passes` | `GET` | List all guest passes (optionally filter client-side by name in the UI). |
| `/api/passes` | `POST` | Create a pass: `{ name, durationHours, shareNumber }`. |
| `/api/passes/daily` | `GET` | Today's pass. Falls back to the last pass this app successfully resolved when the controller is unreachable, so a controller blip doesn't blank the guest display — the code printed on the wall stays valid either way. The `X-Daily-Pass-Current` response header is `false` when the pass served is stale or came from that cache. `404` when the controller answered but there is no daily pass at all; `503` when nothing is known yet (a cold start during an outage). |
| `/api/health` | `GET` | See below. |

`/api/health` **deliberately always returns HTTP 200**, even when the
Unleashed controller is completely unreachable. Restarting this app cannot
fix an upstream controller outage, so failing liveness/readiness on it
would just turn a controller outage into a crash-loop or pull the whole
app out of service — making a partial outage total. Instead, the body
reports the truth:

```json
{ "status": "degraded", "dailyPassCurrent": false, "controllerReachable": false }
```

It answers from an in-memory snapshot that a background task refreshes
every 60s, and never waits on the controller itself — a probe's
`timeoutSeconds` is always far below the 30s controller client timeout, so
a blocking health check would fail the probe on timeout during an outage
and cause the exact crash-loop described above. The trade-off is that
`controllerReachable` can lag reality by up to a minute.

**If you want a probe that actually fails when the controller is down**,
there isn't one by design, and that is deliberate — every endpoint here
either keeps serving the last-known-good answer or reports the problem in
its body. Alert on `controllerReachable: false` from `/api/health` (or on
`dailyPassCurrent: false`, which also catches a roll that silently didn't
happen) rather than wiring it to a Kubernetes probe.

## Troubleshooting

- **Old passes piling up in the controller's guest list.** Expected — see
  constraint #1. Prune them from the Unleashed admin UI directly; this app
  cannot delete.
- **Yesterday's code still works after today's was minted.** The guest WLAN
  is on first-use validity. Switch it to "Effective from the creation time"
  on that SSID — see [Controller setup](#controller-setup). The change is
  retroactive, so existing codes fall in line too.
- **One guest connects and then nobody else can use the code.** Check
  `DAILY_SHARE_NUMBER` — it's probably set to `1`. Use `0` for unlimited.
- **WiFi QR code isn't showing on `/display`.** Confirm both `WIFI_SSID`
  and `WIFI_PASSWORD` are set (an empty string is fine for `WIFI_PASSWORD`
  on an open network, but the variable must exist). Check the browser
  console for a warning.
- **Backend can't reach the controller / `AD_PrivilegeInsufficient` on
  create.** Verify `UNLEASHED_URL`, `UNLEASHED_HAS_VALID_CERT`, and that
  the account is a Guest Pass Manager with guest-pass creation rights on
  the target SSID. Increase `BACKEND_LOG_LEVEL=debug` for more detail.
- **Health check says "degraded".** The controller is unreachable from the
  backend's perspective — check network/DNS/firewall between this app and
  `UNLEASHED_URL`. The app itself is still up and serving the last-known
  pass.

## Attribution

This project is a rewrite of
[etiennecollin/unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
retargeted from UniFi controllers to Ruckus Unleashed. The Unleashed
guest-pass protocol was worked out with reference to
[fmuffat/FetchPass](https://github.com/fmuffat/FetchPass) and
[ms264556/aioruckus](https://github.com/ms264556/aioruckus). See
[NOTICE](./NOTICE) for full license attribution and [LICENSE](./LICENSE)
for this project's MIT license (unchanged from upstream).

This is an independent, unofficial project. It is not affiliated with,
endorsed by, or sponsored by CommScope, RUCKUS Networks, or Ubiquiti Inc.
Ruckus, Unleashed, UniFi, Ubiquiti, and all associated trademarks, logos,
and intellectual property are the property of their respective owners.
Their use here is for identification and interoperability purposes only.
