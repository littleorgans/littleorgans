# Postgres

`lilo` persists all daemon state in Postgres. This page is the operator setup
guide for the three target environments (local native, Docker Compose, and
cloud managed) and the deterministic smoke that proves a connection works.

> **Status:** v0.8 pre-release. Postgres is the single supported backend; there
> is no SQLite fallback and no automatic migration from any older local state.

The database connection is one variable, `LILO_DATABASE_URL`. Its full contract
(audience, precedence, the `settings.toml` overlay, and the related test and
Compose variables) lives in [`env-vars.md`](env-vars.md); this page does not
restate those definitions, it shows how to satisfy them per environment.

## How it connects

- `LILO_DATABASE_URL` is a standard Postgres connection URL and has **no**
  built-in default. If it is unset, `lilo` reads `[database] url` from
  `$LILO_HOME/settings.toml`; if neither is set the daemon fails with a clear
  operator error. The environment variable wins over `settings.toml`.
- The URL is parsed by the driver (`sqlx`), so any standard parameter you put on
  it (for example `?sslmode=require`) is honored. `lilo` bakes in no
  provider-specific behavior.
- **Migrations run automatically** on the first connection. The daemon, the CLI
  health check, and the test fixture all apply the embedded migration set on
  open; there is no separate migrate command to run.
- Connection and migration errors name the failed `host:port/database` target
  with the password redacted.

## Local native

Run a Postgres server directly on the host.

1. Install and start Postgres 17 (match the version used by Compose and CI):

   ```sh
   # macOS (Homebrew)
   brew install postgresql@17 && brew services start postgresql@17

   # Debian/Ubuntu
   sudo apt-get install -y postgresql-17 && sudo systemctl enable --now postgresql
   ```

2. Create a role and database for `lilo`:

   ```sh
   createuser --pwprompt lilo          # enter a password when prompted
   createdb --owner lilo lilo
   ```

3. Point `lilo` at it (export in your shell, or use `settings.toml` below):

   ```sh
   export LILO_DATABASE_URL="postgres://lilo:PASSWORD@localhost:5432/lilo"
   ```

   To avoid keeping the URL in the environment, copy `settings.example.toml` to
   `$LILO_HOME/settings.toml` and set `[database] url` there instead. An explicit
   `LILO_DATABASE_URL` always overrides the file.

4. Verify with the [smoke](#smoke) below.

## Docker Compose

The repo ships a `compose.yaml` with a health-checked `postgres:17` service.
This is the same service used for `lilo-db` development and the Postgres-backed
test suite.

```sh
docker compose up -d --wait postgres
export LILO_DATABASE_URL="postgres://lilo:lilo@localhost:56432/lilo"
```

The service binds container port `5432` to `127.0.0.1:56432` by default. To let
Docker allocate a free host port, set `LILO_DATABASE_DOCKER_PORT` to `0`, then
read the assigned endpoint and set the matching URL:

```sh
LILO_DATABASE_DOCKER_PORT=0 docker compose up -d --wait postgres
database_endpoint="$(docker compose port postgres 5432)"
export LILO_DATABASE_URL="postgres://lilo:lilo@${database_endpoint}/lilo"
```

For a different fixed port, set `LILO_DATABASE_DOCKER_PORT` before Compose and
use the same port in `LILO_DATABASE_URL`. The Compose variable changes only the
published host port. `lilo` does not read it.

The named volume holds disposable local data and can be removed at any time
(`docker compose down -v`).

## Cloud managed

Any managed Postgres that speaks the standard wire protocol works; supply its
connection URL and `lilo` treats it like any other Postgres.

```sh
export LILO_DATABASE_URL="postgres://USER:PASSWORD@HOST:5432/DBNAME?sslmode=require"
```

TLS expectations:

- Most managed providers require TLS. Request it with the standard `sslmode`
  parameter on the URL: `?sslmode=require` to encrypt, or
  `?sslmode=verify-full&sslrootcert=/path/to/ca.pem` to also verify the server
  certificate against a CA bundle.
- These parameters are interpreted by the driver, not by `lilo`. No
  provider-specific code paths exist in the core; if your provider speaks
  standard Postgres, a standard URL is all that is needed.
- The role named in the URL must own (or be able to create) the schema so the
  automatic migrations can apply on first connection.

## Smoke

A deterministic, bounded check that the configured database is reachable and
fully migratable. Run it after any of the setups above:

```sh
export LILO_DATABASE_URL="postgres://...";  # local, compose, or cloud URL

lilo doctor                       # reports `db: ok` when the connection works
lilo daemon start --ready-check   # brings the daemon fully up, then exits 0
```

`lilo daemon start --ready-check` opens the database, runs migrations, binds the
daemon socket, writes the pidfile, confirms readiness, then cleanly shuts down
and exits `0`, leaving no socket or pidfile behind. It is bounded and
deterministic, so it is safe to use as a CI or provisioning gate. A non-zero
exit means the database is unreachable, unmigratable, or misconfigured. Without
`--ready-check`, `lilo daemon start` runs the daemon in the foreground until it
receives a shutdown signal.

Expected output:

```text
$ lilo doctor
db: ok
...

$ lilo daemon start --ready-check
lilod ready-check ok: database connected, migrations applied, socket bound; shut down cleanly
```

`--output json` renders the ready-check result as `{"ok":true}`.

## Tests

The Postgres-backed tests across the workspace are `#[ignore]`d so the default
suite skips them. Run them against a real database with:

```sh
docker compose up -d --wait postgres
LILO_TEST_DATABASE_URL="postgres://lilo:lilo@localhost:56432/lilo" just test-db
```

`LILO_TEST_DATABASE_URL` is the admin connection the `lilo-db` fixture uses to
create and drop isolated throwaway databases per test; it resolves over
`LILO_DATABASE_URL` and the `settings.toml` `[database] test_url`/`url` keys. CI
sets it to the same health-checked `postgres:17` service and runs the suite
automatically.
