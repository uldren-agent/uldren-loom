# PIM Reference-Client Certification Harness

This harness creates a deterministic local PIM certification fixture for Queue 7.

It seeds a `.loom` store with:

- account: `example@uldrentest.com`
- password: `testpassword`
- three calendar resources
- three contacts
- three mail messages
- subscribed IMAP role mailboxes: Inbox, Archive, Drafts, Junk, Notes, Sent, and Trash
- a Loom-hosted setup-only SMTP compatibility listener for reference-client account setup

It also generates a local root CA plus a server certificate for `uldrentest.com`, imports the server
certificate bundle into the loom, configures durable hosted listeners, and starts the daemon on ports
that reference clients can use.

## Ports

CalDAV and CardDAV are configured as separate served surfaces, but the daemon coalesces matching DAV
records into one HTTPS listener when their bind, TLS, auth, limits, audit, exposure, and network
policy match.

| Surface | Host | Port | URL |
| --- | --- | ---: | --- |
| CalDAV | `uldrentest.com` | 443 | `https://uldrentest.com/.well-known/caldav` |
| CardDAV | `uldrentest.com` | 443 | `https://uldrentest.com/.well-known/carddav` |
| IMAPS | `uldrentest.com` | 993 | `uldrentest.com:993` |
| SMTP submission | `uldrentest.com` | 587 | STARTTLS, auth required |
| JMAP | `uldrentest.com` | 8444 | `https://uldrentest.com:8444/jmap/session` |

## Build

Build and pin the harness CLI before configuring or starting listeners:

```sh
scripts/pim-cert/build-local-loom.sh
```

The harness scripts default to `scripts/pim-cert/bin/loom`. Set `LOOM_BIN=/path/to/loom` only when
you intentionally want to override that pinned copy.

## Create the Fixture

```sh
scripts/pim-cert/generate-ca.sh
scripts/pim-cert/seed.sh
scripts/pim-cert/configure-listeners.sh
```

By default outputs are written under `scripts/pim-cert/out/`.

Set `RESET=1` when you intentionally want to replace the generated fixture:

```sh
RESET=1 scripts/pim-cert/seed.sh
RESET=1 scripts/pim-cert/generate-ca.sh
```

## Trust The Local Root CA On macOS

After `generate-ca.sh`, install the local root CA:

```sh
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain scripts/pim-cert/out/certs/ca.cert.pem
```

To remove it later:

```sh
sudo security delete-certificate -c "Uldren Loom Local Test Root CA" /Library/Keychains/System.keychain
```

## Start And Stop

Start on non-standard loopback ports first:

```sh
scripts/pim-cert/start-local-ports.sh
```

That configures and starts:

| Surface | Host | Port | URL |
| --- | --- | ---: | --- |
| CalDAV | `uldrentest.com` | 10443 | `https://uldrentest.com:10443/.well-known/caldav` |
| CardDAV | `uldrentest.com` | 10443 | `https://uldrentest.com:10443/.well-known/carddav` |
| IMAPS | `uldrentest.com` | 10993 | `uldrentest.com:10993` |
| SMTP submission | `uldrentest.com` | 1587 | STARTTLS, auth required |
| JMAP | `uldrentest.com` | 18444 | `https://uldrentest.com:18444/jmap/session` |

The SMTP listener is a Loom-hosted local certification shim. It accepts `STARTTLS`, authenticates
`example@uldrentest.com` with the fixture password, and accepts submitted `DATA` only so account setup
and send-probe flows can complete. It does not relay, deliver, or mutate the mail facet.

Stop the local daemon and hosted listeners with:

```sh
scripts/pim-cert/stop-local-ports.sh
```

Run the local live RFC probes after the daemon is running:

```sh
scripts/pim-cert/rfc-probe.sh
```

The probe writes a redacted summary to `scripts/pim-cert/out/manual-results/live-rfc-probes.json`.
It verifies the bounded CalDAV, CardDAV, IMAP, JMAP, and SMTP STARTTLS fixture paths over the live
daemon. It is local protocol evidence, not external Apple, Thunderbird, or DAVx5 certification.

After local ports are verified, standard ports can be configured and started. Starting on ports
`443` and `993` needs elevation. The standard-port script also starts SMTP compatibility on port `587`:

```sh
scripts/pim-cert/configure-listeners.sh
scripts/pim-cert/start-standard-ports.sh
scripts/pim-cert/stop-standard-ports.sh
```

## Android Emulator Hostname

Android emulators should resolve `uldrentest.com` to the host loopback bridge `10.0.2.2`, not to
`127.0.0.1` inside the emulator. Try:

```sh
scripts/pim-cert/android-hosts.sh
```

That script requires a root-capable emulator image. If it fails, configure the DAVx5 account with a
hostname override only if your installed DAVx5 build supports it; otherwise use Apple and Thunderbird
evidence first and record DAVx5 as skipped with the reason.

## Manual Evidence

Use `client-checklist.md` for manual verification. Write results into
`out/manual-results/YYYYMMDD-client.md` or attach exported client logs there.
