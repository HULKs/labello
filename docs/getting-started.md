# Getting started

Labello runs as two processes during development: an API server and a Trunk
server for the browser client. Use the same hostname for both throughout login.

## Start the application

Install Rustup. From the repository root, install the pinned compiler and Trunk:

```sh
rustup show
cargo install --locked trunk --version 0.21.14
cargo run --locked -p labello-server
```

The repository selects Rust 1.98.0, rustfmt, Clippy, and
`wasm32-unknown-unknown`. Native inspector builds additionally need the graphics
libraries listed in [verification](verification.md#canonical-entry-point).

The API creates `labello.server.toml` if it is missing, uses `datasets/` for data,
and listens at `127.0.0.1:8080`. In a second terminal:

```sh
cd apps/labello-wasm
trunk serve --locked --address 127.0.0.1 --port 8081
```

Open <http://127.0.0.1:8081>. The login page checks the session and available
sign-in methods before showing them. Choose **Continue as local admin** for the
local setup. This signs in as the first bootstrap administrator, `admin` by
default. Keep this login disabled on an internet-facing installation.

## Create the first dataset

1. In Setup, create a dataset with a new ID and name. Only bootstrap
   administrators can create datasets.
2. Leave **Copy schema from** at None to define new classes and workflows, or
   select a dataset where you have DataAdmin access. Check its preview before
   creation; schema copies retain compatible IDs and keypoint ordering.
3. In Administration, configure classes and box or skeleton workflows. Choose
   whether submission completes work or sends it to approval review.
4. Add images through a browser folder or configured relative filesystem roots,
   then ingest them. Ingestion identifies duplicates by content hash.
5. Assign annotator, reviewer, and data-admin roles as needed.
6. Open Annotate, select a workflow, and label the assigned image. Use Review
   for submitted work and Inspect to browse images without claiming them.

See [administration](administration.md) for schema-copy boundaries and dataset
settings, and [annotation and review](annotation.md) for workspace controls.
To bring existing labels into a new dataset, use [Import a dataset](import.md)
when the server advertises that capability.

## Connect and sign in

The browser resolves its API URL from the `api` query parameter, then public
`labello.client.json`, then port 8080 on the browser's hostname. Configure a
hosted default using the [browser configuration](configuration.md#browser-runtime-configuration).
Advanced connection on the login page permits an endpoint override.

GitHub OAuth requires the server settings and callback described in
[configuration](configuration.md#github-oauth). GitHub accounts use internal
IDs such as `github_123456`. On first login they receive annotator access to
existing datasets without a role assignment; data admins can change that access.
Dataset creation still requires inclusion in `bootstrapAdmins`.

The browser prepares two upcoming assignments by default. `queueSize=1` holds
one upcoming assignment; the supported range is 1 through 2, in addition to
current work.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| No login method | Confirm the server enables local development login or GitHub OAuth; retry failed option discovery separately from the session check |
| Browser cannot reach the API | Confirm both processes are running, the API URL is correct, and `browserOrigins` contains the exact browser origin |
| OAuth returns without a session | Keep `localhost` and `127.0.0.1` consistent; verify the public callback path and proxy prefix |
| Startup reports invalid browser configuration | Check JSON syntax, the absolute URL, and a trailing slash on path prefixes; missing runtime files must return 404, not `index.html` |
| No assignment | Check the selected workflow, role, enabled tasks, active leases, imported coverage, and [balance window](assignment.md#completion-balance) |
| Image load fails | Use Retry image load; the browser never fetches a larger image as a fallback |
| A request fails | Record the displayed request ID and consult redacted server logs |

About is available before and after sign-in and can copy browser/server build
information. For production installation and backups, continue with
[deployment](deployment.md) and [operations](operations.md).
