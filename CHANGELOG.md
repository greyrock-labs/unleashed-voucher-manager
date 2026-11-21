# Changelog

## [1.0.1] - 2026-11-21

### Bug Fixes

- **deps:** Update react monorepo (#3)

## [1.0.0] - 2026-11-20

### Features

- **backend:** Parse the Unleashed guest list
- **backend:** Build and validate Unleashed guest pass creates
- **backend:** Add an Unleashed admin session with re-login
- **backend:** Map guest passes to vouchers and rolling rules
- **backend:** Replace the UniFi client with Unleashed
- **frontend:** Adapt voucher creation, display and gating to Unleashed
- **frontend:** Use the RUCKUS dog for the logo, QR code and icons

### Bug Fixes

- **backend:** Back off at startup, survive DST midnights, redact the password
- **backend:** Say why the controller could not be reached
- **frontend:** Explain rejected creates and keep Custom Create consistent
- **backend:** Refuse an unusable URL or rolling duration at startup
- **backend:** Confirm deletes against the guest list
- **frontend:** Dim disabled form fields
