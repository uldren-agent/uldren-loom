# PIM Reference-Client Manual Checklist

Fixture account:

- username: `example@uldrentest.com`
- password: `testpassword`

## Server Endpoints

| Client area | Endpoint |
| --- | --- |
| CalDAV | `https://uldrentest.com/.well-known/caldav` |
| CardDAV | `https://uldrentest.com/.well-known/carddav` |
| IMAPS | `uldrentest.com`, port `993`, SSL/TLS |
| SMTP | `uldrentest.com`, port `587`, STARTTLS |
| JMAP | `https://uldrentest.com:8444/jmap/session` |

## Authentication Notes

IMAP, CalDAV, and CardDAV accept the principal display name as username, so use
`example@uldrentest.com`.

CalDAV and CardDAV use HTTPS Basic auth against the Loom passphrase verifier in this harness.
The Loom-hosted SMTP compatibility listener accepts `STARTTLS` plus `AUTH PLAIN` or `AUTH LOGIN` for
the same fixture credentials, then accepts submitted `DATA` for setup compatibility. It does not relay,
deliver, or mutate the mail facet.

## Expected Seed Data

Calendar:

- `PIM cert kickoff`
- `Reference client review`
- `Protocol transcript signoff`

Contacts:

- `Ada Lovelace`
- `Grace Hopper`
- `Katherine Johnson`

Mail:

- `PIM cert message one`
- `PIM cert message two`
- `PIM cert message three`

## Apple Calendar

- Add CalDAV account using `https://uldrentest.com/.well-known/caldav`.
- Confirm the three expected calendar events appear.
- Create one event named `Apple Calendar writeback`.
- Confirm it appears after refresh.
- Record result in `out/manual-results/apple-calendar.md`.

## Apple Contacts

- Add CardDAV account using `https://uldrentest.com/.well-known/carddav`.
- Confirm the three expected contacts appear.
- Create one contact named `Apple Contacts Writeback`.
- Confirm it appears after refresh.
- Record result in `out/manual-results/apple-contacts.md`.

## Apple Mail

- Add IMAP account.
- Email address: `example@uldrentest.com`.
- Username: `example@uldrentest.com`.
- Password: `testpassword`.
- Incoming server: `uldrentest.com`, port `993`, SSL/TLS.
- Outgoing server: `uldrentest.com`, port `587`, STARTTLS, normal password.
- Confirm the three expected messages appear in Inbox.
- Mark one message read and one flagged if the UI supports it.
- Record whether setup required SMTP in `out/manual-results/apple-mail.md`.

## Thunderbird Calendar And Contacts

- Add CalDAV calendar from `https://uldrentest.com/.well-known/caldav`.
- Add CardDAV address book from `https://uldrentest.com/.well-known/carddav`.
- Confirm the three expected events and contacts appear.
- Create one event and one contact.
- Record result in `out/manual-results/thunderbird-dav.md`.

## Thunderbird Mail

- Add IMAP account.
- Email address: `example@uldrentest.com`.
- Username: `example@uldrentest.com`.
- Password: `testpassword`.
- Incoming server: `uldrentest.com`, port `993`, SSL/TLS.
- Outgoing server: `uldrentest.com`, port `587`, STARTTLS, normal password.
- Confirm the three expected messages appear in Inbox.
- Record whether setup required SMTP in `out/manual-results/thunderbird-mail.md`.

## DAVx5

- Ensure Android resolves `uldrentest.com` to `10.0.2.2` using `scripts/pim-cert/android-hosts.sh`.
- Install DAVx5 with `scripts/pim-cert/install-davx5.sh` when an APK path is available.
- Add CalDAV and CardDAV services:
  - CalDAV: `https://uldrentest.com/.well-known/caldav`
  - CardDAV: `https://uldrentest.com/.well-known/carddav`
- Confirm the three expected events and contacts appear.
- Create one event and one contact.
- Record result in `out/manual-results/davx5.md`.
