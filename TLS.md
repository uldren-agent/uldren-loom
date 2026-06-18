# TLS-backed Loom admin listener

This file describes the local smoke path for running the Loom admin surface over direct TLS on
`127.0.0.1:9999`.

## What it starts

`./tls.sh` creates or reuses:

- `tmp/tls/admin.loom`, a local unencrypted test store;
- `tmp/tls/localhost.crt`, a self-signed localhost certificate;
- `tmp/tls/localhost.key`, the matching private key;
- one durable served listener record for `admin/rest` bound to `127.0.0.1:9999`.

The generated private key is kept under `tmp/`, which is ignored by git. Do not copy this key into a
shared deployment. It is only a local smoke-test credential.

## Run it

```sh
./tls.sh
./tls.sh start
```

The script builds `target/debug/loom` if needed, initializes the test store if needed, configures the
admin listener with direct TLS, starts or restarts the daemon, waits for the HTTPS endpoint, and then
verifies it with:

```sh
curl --fail --silent --show-error --insecure https://127.0.0.1:9999/admin/listeners
```

On success, the daemon is left running and the script prints the listener JSON returned by the admin
surface.

## Stop it

```sh
./tls.sh stop
```

## Source-backed boundary

This smoke path exercises the current source-backed HTTP TLS boundary:

- `admin/rest` is served by the local daemon from durable `loom serve` configuration.
- Direct TLS loads PEM certificate and key files.
- The current certificate is self-signed, so the smoke curl uses `--insecure`.
- Trust-bundle loading, mTLS, and gRPC direct TLS are still target work in the queue.
