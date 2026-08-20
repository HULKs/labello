# Debian 12 deployment guest source

The files in `github-runner-base/` come from
`HULKs/hulk/tools/ci/github-runners` at commit
`6c6cec2f8ef8023987e036af783bf34544aac2cf`. That is the source used to
create the current Labello runner guest.

- `actions-runner.yaml` builds the Debian 12 Bookworm amd64 LXC root
  filesystem with systemd, SSH, Docker, and GitHub Actions runner
  prerequisites.
- `Containerfile` defines the existing runner's general Rust build container
  and is retained as part of the exact upstream reproduction input. Production
  Labello binaries must instead use `deployment/release/Containerfile`, whose
  digest-pinned Bookworm base keeps its glibc baseline compatible with the
  Debian 12 guest. Build and publish that image, then configure
  `LABELLO_RELEASE_BUILD_IMAGE` with its immutable digest. Do not put private
  registry paths in this repository.

Build the guest root filesystem from the pinned YAML with `distrobuilder`:

```sh
distrobuilder build-lxc --compression zstd \
  deployment/guest/github-runner-base/actions-runner.yaml
```

Create a privileged Proxmox LXC with nesting enabled, as required by the
source template. Allocate storage for the complete Labello data root, one
verified backup, release generations, and operational headroom. Do not record
the guest hostname, address, runner name, or runner label in Git.

Limit inbound application traffic to HTTP/HTTPS for Caddy. The Actions runner
needs no inbound port, and operator SSH should follow the normal restricted
management policy. Allow outbound DNS and HTTPS for GitHub releases/API/assets,
Sigstore verification, and the configured TLS certificate authority. Keep the
actual firewall rules, addresses, and management networks in private
infrastructure configuration.

The base image initially places `hulk` in the `sudo` and `docker` groups. That
is appropriate for the general build runners but gives the account effective
root access. The Labello deployment runner does not use Docker or sudo. After
the runner is registered and the one-time host setup is complete, remove
`hulk` from both groups and disable Docker unless the guest has a separately
documented need for it:

```sh
deluser hulk sudo
deluser hulk docker
systemctl disable --now docker.service docker.socket
```

Install the host packages as root during guest provisioning. The deployment
workflow never installs packages:

```sh
apt-get update
apt-get install --yes --no-install-recommends ca-certificates caddy curl gzip tar
systemctl disable --now caddy.service
```

Create the stable, mount-ready data location and hand the deployment tree to
the runner account:

```sh
install -d -o hulk -g hulk -m 0750 /var/lib/labello
install -d -o hulk -g hulk -m 0750 \
  /var/lib/labello/bin \
  /var/lib/labello/backups \
  /var/lib/labello/caddy \
  /var/lib/labello/caddy/config \
  /var/lib/labello/caddy/data \
  /var/lib/labello/caddy/live \
  /var/lib/labello/caddy/maintenance \
  /var/lib/labello/config \
  /var/lib/labello/config/browser \
  /var/lib/labello/configurations \
  /var/lib/labello/data \
  /var/lib/labello/releases \
  /var/lib/labello/requests \
  /var/lib/labello/state
```

`/var/lib/labello/data` is the complete `datasetsRoot`. A later block-device
or storage mount can replace that directory at the same path. Mount it with
the numeric UID and GID used by `hulk`, and test ownership before starting the
server.

User services need a persistent user manager, and rootless Caddy needs access
to ports 80 and 443. Apply these once as root:

```sh
loginctl enable-linger hulk
printf '%s\n' 'net.ipv4.ip_unprivileged_port_start=80' \
  > /etc/sysctl.d/90-labello-rootless-ports.conf
sysctl --load /etc/sysctl.d/90-labello-rootless-ports.conf
```

Build `labello-deploy` from the exact reviewed Labello revision using the
pinned release build image, then install it as `hulk` at
`/var/lib/labello/bin/labello-deploy`. Copy the tracked Caddy templates to
their matching directories below `/var/lib/labello/caddy`. Install the four
tracked unit files in `/home/hulk/.config/systemd/user`.

Before starting the services, create the deployment-specific configuration.
Keep these values untracked:

- `/var/lib/labello/config/caddy.env` contains only
  `LABELLO_SITE_ADDRESS=<public HTTPS site address>`.
- `/var/lib/labello/config/browser/labello.client.json` contains
  `{"apiBaseUrl":"https://<public site>/api/"}`.
- `/var/lib/labello/config/labello.server.toml` contains the production server
  configuration and references `/var/lib/labello/data` as `datasetsRoot`.
- `/var/lib/labello/config/labello-server.env` may contain OAuth environment
  overrides. Set mode `0600` on files that contain credentials.

Then run as `hulk`:

```sh
ln -s maintenance /var/lib/labello/caddy/current
systemctl --user daemon-reload
systemctl --user enable --now labello-deploy-recover.service
systemctl --user enable labello-server.service
```

The server unit is safe to enable before the first release: systemd skips it
until the current release executable exists. Keep the Caddy user service
disabled while the public name resolves elsewhere. After DNS points to this
guest and inbound ports 80 and 443 reach it, start the maintenance gateway:

```sh
systemctl --user enable --now labello-caddy.service
```

This ordering avoids failed ACME validation and certificate-authority retry
limits during pre-cutover setup.

At the start of a transaction, `labello-deploy` copies this source directory
to `configurations/<release>` and switches `configurations/current` with the
matching executable generation. Edit only `config/`, never a published
configuration generation.

Register the runner with a private label and store that label only in the
repository variable `LABELLO_DEPLOY_RUNNER`. A repository-scoped runner cannot
join an organization runner group. When an organization owner is available,
re-register it at organization scope and restrict its group to
`HULKs/labello/.github/workflows/deploy.yml@refs/heads/main`.

Verify the final boundary without printing configuration or credentials:

```sh
systemctl is-active 'actions.runner.*'
sudo -n -l
id -nG
systemctl --user status labello-deploy-recover.service
```

The runner account must not retain sudo or Docker access. Routine deployment
must be able to mutate only `/var/lib/labello` through ordinary ownership and
control its own user services.
