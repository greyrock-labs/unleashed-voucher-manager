# Unleashed Voucher Manager

A touch-friendly web application for managing guest passes on Ruckus Unleashed
controllers: create them, print them, show the current one on a kiosk, and
roll a new one automatically each time a guest connects.

It is a port of
[unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
by Etienne Collin to Ruckus Unleashed, and keeps its interface and features
wherever the controller allows.

<!-- vim-markdown-toc GFM -->

- [Features](#features)
- [Quick start](#quick-start)
  - [Docker Compose](#docker-compose)
  - [Without Docker](#without-docker)
- [Configuration](#configuration)
  - [Controller account](#controller-account)
  - [Rolling vouchers and the kiosk page](#rolling-vouchers-and-the-kiosk-page)
  - [Custom SVG logo](#custom-svg-logo)
  - [Environment variables](#environment-variables)
- [Differences from UniFi Voucher Manager](#differences-from-unifi-voucher-manager)
- [Troubleshooting](#troubleshooting)
- [Credits](#credits)

<!-- vim-markdown-toc -->

## Features

- **Quick Create**: a voucher with a preset duration, from 1 hour to 30 days.
- **Custom Create**: choose the number of vouchers (up to 100 at once), name,
  duration in hours, days or weeks, guest limit, a custom key and remarks.
- **Browse vouchers**: search by name, view details, select and delete in
  bulk, delete expired vouchers.
- **Print vouchers**: list or grid layout, friendly to thermal printers.
- **WiFi QR code**: lets guests join the network by scanning.
- **Rolling vouchers**: a new voucher is created for the next guest whenever
  the current one is used.
- **Kiosk page** (`/kiosk`): the current rolling voucher and the WiFi QR code,
  updated live.
- **Interface**: touch-friendly, dark and light mode, notifications, custom
  logo.
- **Architecture**: a Next.js frontend and an Axum (Rust) backend in one
  container. Only the backend talks to the controller, so the controller
  credentials never reach the browser.

## Quick start

### Docker Compose

1. Download `compose.yaml` from this repository.
2. Set the environment variables in it (see
   [Environment variables](#environment-variables)).
3. Start it:

   ```bash
   docker compose up -d --force-recreate
   ```

4. Open `http://localhost:3000`.

### Without Docker

1. Install `rust >= 1.88.0`, `nodejs >= 24.3.0` and `npm >= 11.4.2`.
2. Clone this repository.
3. Set the environment variables in your shell, or in a `.env` file at the
   repository root and use the backend's `dotenv` feature.
4. Start the backend and frontend:

   ```bash
   # Backend (without a .env file)
   cd backend && cargo run --release

   # Backend (with a .env file)
   cd backend && cargo run --release --features dotenv

   # Frontend (development)
   cd frontend && npm install && npm run dev

   # Frontend (release)
   cd frontend && npm ci && npm run build && npm run start
   ```

5. Open `http://localhost:3000`.

## Configuration

### Controller account

The app logs in to the controller's admin interface with a username and
password. The account needs a role with **read-write admin privilege**.
Unleashed's built-in Guest Pass Manager role can list and create guest passes
but cannot delete them, so delete, cleanup and rolling vouchers would not
work with it.

In the Unleashed web interface, create a role with read-write admin
privilege, then a user in that role, and give that user's credentials to the
app.

### Rolling vouchers and the kiosk page

Rolling vouchers give each guest a fresh code: when a guest uses the current
rolling voucher, the app creates the next one, and the kiosk page shows it.

> [!IMPORTANT]
> Rolling vouchers need the guest WLAN to send guests to the app after they
> log in. In the Unleashed web interface, open the guest WLAN's guest access
> (captive portal) settings and set **Redirect to the following URL** to the
> app's `/welcome` page, for example `https://voucher.example.com/welcome`.
>
> Without this, vouchers do not roll when guests connect.

> [!CAUTION]
> Set `GUEST_SUBNETWORK` to the guest network's subnet. Guests on it can then
> reach only `/welcome`, not the voucher management pages. Without it, guests
> can create and delete vouchers themselves.

How it works:

1. The kiosk page shows the current rolling voucher, creating one if none
   exists.
2. A guest logs in with it and the captive portal sends them to `/welcome`.
3. `/welcome` asks the backend for the next rolling voucher. Only one unused
   rolling voucher ever waits: if one already exists, the backend returns it
   instead of creating another, so reloading the page does not create more.
4. Rolling vouchers are named `[ROLLING]-<timestamp>-<ip>`, after the address
   that triggered them.
5. When the guest portal counts a pass's time from when it was issued, an
   unused rolling voucher can expire on the kiosk. The controller then
   removes it, and the kiosk, which re-checks every minute, creates a new
   one.
6. At midnight (in `TIMEZONE`) expired rolling vouchers are deleted, or all
   expired vouchers if `PURGE_ALL_EXPIRED_VOUCHERS` is set. Unleashed usually
   removes expired passes itself, so this rarely finds anything.

The app takes the client's address from the **last** entry of
`X-Forwarded-For`, so the reverse proxy directly in front of it must append
(or set) the address it received the connection from. Earlier entries come
from the client and are ignored, because a guest could forge them. Do not
expose port 3000 to the guest network directly. `GUEST_SUBNETWORK` depends
on this.

### Custom SVG logo

The header shows `logo.svg`, and the centre of the WiFi QR code shows
`qr-logo.svg` (the RUCKUS dog by default). Either can be replaced:

- With Docker, mount the SVG at `/app/frontend/public/logo.svg` or
  `/app/frontend/public/qr-logo.svg`. There is an example in `compose.yaml`.
  The mount destination, including the file name, **cannot be changed**.
- Without Docker, place the SVG at `./frontend/public/logo.svg` or
  `./frontend/public/qr-logo.svg`.

The QR code is drawn in the page's text colour on a transparent background,
so give `qr-logo.svg` its own background to keep it visible in both themes.

### Environment variables

Required variables come first.

- **`UNLEASHED_URL`: `string`** (_Required_)
  - URL of the Unleashed controller, with protocol.
  - Example: `https://unleashed.example.com`
- **`UNLEASHED_USERNAME`: `string`** (_Required_)
  - A controller user whose role has read-write admin privilege. See
    [Controller account](#controller-account).
- **`UNLEASHED_PASSWORD`: `string`** (_Required_)
  - That user's password.
- **`UNLEASHED_SSID`: `string`** (_Required_)
  - The guest WLAN that vouchers are created on, as named on the controller.
  - Example: `Guest WiFi`
- **`UNLEASHED_HAS_VALID_CERT`: `bool`** (_Optional_)
  - Whether the controller's certificate is trusted. Set to `false` if it
    serves its default self-signed certificate.
  - Default: `true`
- **`GUEST_SUBNETWORK`: `IPv4 CIDR`** (_Optional_)
  - Guests on this subnet can reach only `/welcome`. See
    [Rolling vouchers and the kiosk page](#rolling-vouchers-and-the-kiosk-page).
  - Example: `10.0.5.0/24`
- **`TIMEZONE`: [timezone identifier](https://en.wikipedia.org/wiki/List_of_tz_database_time_zones#List)**
  (_Optional_)
  - Used to show dates and to schedule the midnight cleanup.
  - Default: `UTC`
- **`ROLLING_VOUCHER_DURATION_MINUTES`: `minutes`** (_Optional_)
  - How long a rolling voucher grants access. Rounded up to whole hours,
    because the controller only accepts hours, days and weeks.
  - Default: `480`
- **`PURGE_ALL_EXPIRED_VOUCHERS`: `bool`** (_Optional_)
  - When `true`, the midnight cleanup deletes all expired vouchers; when
    `false`, only expired rolling vouchers.
  - Default: `false`
- **`WIFI_SSID`: `string`** (_Optional_)
  - SSID for the QR code. The QR code needs both `WIFI_SSID` and
    `WIFI_PASSWORD`.
- **`WIFI_PASSWORD`: `string`** (_Optional_)
  - Password for the QR code. Set to `""` for an open network.
- **`WIFI_TYPE`: `WPA|WEP|nopass`** (_Optional_)
  - Security type for the QR code. Defaults to `WPA` with a password and
    `nopass` without.
- **`WIFI_HIDDEN`: `bool`** (_Optional_)
  - Whether the SSID is hidden.
  - Default: `false`
- **`IS_LOGO_INVERTIBLE`: `bool`** (_Optional_)
  - Whether the logo may be inverted in dark mode.
  - Default: `false`
- **`PRINT_CONFIG`: `JSON object`** (_Optional_)
  - Which fields printed vouchers show. Omitted fields default to `true`.
  - Default:
    `{"showLogo":true,"showDuration":true,"showMaxGuests":true,"showId":true,"showPrintTime":true}`
- **`FRONTEND_BIND_HOST`: `IPv4`** (_Optional_), default `0.0.0.0`
- **`FRONTEND_BIND_PORT`: `u16`** (_Optional_), default `3000`
- **`FRONTEND_TO_BACKEND_URL`: `URL`** (_Optional_), default
  `http://127.0.0.1`
- **`BACKEND_BIND_HOST`: `IPv4`** (_Optional_), default `127.0.0.1`
- **`BACKEND_BIND_PORT`: `u16`** (_Optional_), default `8080`
- **`BACKEND_LOG_LEVEL`: `trace|debug|info|warn|error`** (_Optional_),
  default `info`

## Differences from UniFi Voucher Manager

- **No data or speed limits per voucher.** Unleashed sets bandwidth per WLAN
  or role, not per pass.
- **Durations in whole hours, days or weeks.** The controller cannot create a
  pass shorter than an hour; other values are rounded up to whole hours.
- **Batches of up to 100**, named by the controller (`Guest-1`, `Guest-2`,
  ...).
- **Custom keys and remarks** can be set when creating a voucher.
- **Usernames and passwords** instead of an API key, and an SSID instead of a
  site ID.

## Troubleshooting

- **The backend keeps retrying the connection at startup**
  - It waits 5 seconds, then twice as long each time, up to 5 minutes. The
    log says whether the controller refused the credentials or could not be
    reached.
  - Check `UNLEASHED_URL` is correct and reachable from the container.
  - Check `UNLEASHED_HAS_VALID_CERT` matches the controller's certificate.
  - Check the username and password, and that the role has read-write admin
    privilege.
- **Creating a voucher fails**
  - Check `UNLEASHED_SSID` names an existing guest WLAN.
  - A custom key must be 2 to 16 characters, without spaces or
    `# & + " ' < > ,`, and not already in use.
  - Names and remarks must be plain ASCII. Names cannot contain
    ``' " < > & # ; \ ` | ! $ ( )``, and remarks cannot contain `<` or `>`:
    the controller refuses these, and garbles accented letters.
- **Vouchers do not roll when guests connect**
  - Check the guest WLAN redirects to the app's `/welcome` page.
  - Check the reverse proxy appends the client's address to
    `X-Forwarded-For`.
- **The WiFi QR code button is disabled**
  - Set `WIFI_SSID` and `WIFI_PASSWORD`, and check the browser console for
    configuration errors.

For more detail, run with `BACKEND_LOG_LEVEL="debug"` and check
`docker logs unleashed-voucher-manager`.

## Credits

Based on [unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
by Etienne Collin, under the MIT License. The Unleashed protocol details were
worked out with help from [FetchPass](https://github.com/fmuffat/FetchPass)
and [aioruckus](https://github.com/ms264556/aioruckus).

This is an independent, unofficial project. It is not affiliated with,
endorsed by, or sponsored by Ruckus Networks or Ubiquiti Inc. Ruckus,
Unleashed, UniFi and all associated trademarks are the property of their
respective owners.
