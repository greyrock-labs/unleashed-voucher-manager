# Working against a real Unleashed controller

The verified controller behaviour (endpoints, login, units, limits, name and
remarks characters, error responses) is in the spec's "Verified against the
live controller" section. Read it before changing anything in
`backend/src/unleashed/`. The facts most likely to catch you out:

- A session only works after loading `dashboard.jsp` following login.
- A pass name containing whitespace, or any of ``' " < > & # ; \ ` | ! $ ( )``,
  makes the controller answer `OK` and create nothing. Only a before and after
  comparison of the guest list proves a create worked.
- Minutes are not a valid duration unit; the controller creates a pass that
  expires at once. Use hours, days or weeks.
- `share-number` `0` means unlimited devices; rolling vouchers use `1`.
- Expired passes disappear from the list on their own.

## Live testing rules

Live tests use a real controller that real guests depend on, so:

- **Never test wrong credentials against it.** A few failed logins make it
  refuse further logins from that address for a while, even with the right
  password, and it raises no event or alarm. Unit tests cover the
  wrong-credentials path.
- Ask before any write. Get the credentials from Todd (he keeps them in
  1Password), keep any copy outside the repository with owner-only
  permissions, and delete it when done.
- Record the existing passes first. Only create passes with clearly test-like
  names, delete only the ids you created, and confirm afterwards that the
  list matches what was there before.
- A rolling voucher waiting on the kiosk belongs to the deployment: do not
  use it up or delete it. With one waiting, a rolling request returns it
  without creating anything, which is a safe way to check the rolling path.
- Raw probing is easiest from a short Python script with `requests`: log in
  with `GET /admin/login.jsp?username=...&password=...&ok=Log In`, take the
  `HTTP_X_CSRF_TOKEN` response header, load `dashboard.jsp`, then post to
  `/admin/_conf.jsp` or `/admin/mon_createguest.jsp` with `X-CSRF-Token`. Log
  out afterwards with `GET /admin/login.jsp?logout=1`.
