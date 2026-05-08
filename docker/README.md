# Local DOMjudge stack

A `docker compose` setup that brings up a self-contained DOMjudge installation
suitable for testing the submitter end-to-end: Rust is enabled in the chroot,
printing is configured to dump submitted files into a log file, and the
example "demo" contest is loaded automatically.

## What's in here

| File                                         | Purpose                                                            |
|----------------------------------------------|--------------------------------------------------------------------|
| `docker-compose.yml`                         | Three services: `mariadb`, `domserver`, `judgehost`                |
| `judgehost.Dockerfile`                       | Extends `domjudge/judgehost` and installs `rustc`/`cargo` in chroot |
| `domserver-init/60-enable-rust-and-print.sh` | Init hook: enables `rs`, sets `print_command` to log-tee           |

## Bring it up

```bash
cd docker
docker compose up -d --build       # first run downloads images and builds judgehost+rust
docker compose logs -f domserver   # watch for "DOMjudge installation finished" + admin password
```

The web UI is at <http://localhost:12345/>. The admin password is generated on
first boot and printed into the domserver logs (and saved inside the container
at `/opt/domjudge/domserver/etc/initial_admin_password.secret`):

```bash
docker compose exec domserver cat /opt/domjudge/domserver/etc/initial_admin_password.secret
```

The example "demo" contest is loaded automatically and starts in the future,
so you may need to push its start time into the past via the admin UI
(`Administration → Contests → demo → Edit`) before submitting.

## Submitting

```bash
# Long form (URL from the team UI)
submitter "http://localhost:12345/team/problems/1" "Rust" solution.rs

# Short form (problem letter, picks the active contest)
submitter "http://localhost:12345/A" "Rust" solution.rs
```

When prompted for credentials, use a team account from the demo contest, or
create one in the admin UI. (Default demo accounts have known weak passwords;
check `Users → Show password` as admin if you want to log in as a team.)

## Printing

Anything sent through the team "Print" form is appended to
`/var/log/domjudge/print.log` inside the domserver container. Tail it with:

```bash
docker compose exec domserver tail -f /var/log/domjudge/print.log
# or, if you persist the volume across reboots:
docker compose run --rm domserver tail -f /var/log/domjudge/print.log
```

Each entry is wrapped with timestamped delimiters:

```
=== printed at 2026-05-08T11:23:45Z ===
<file contents>
=== end ===
```

## WSL2 caveat

The judgehost needs working cgroups for runguard sandboxing. WSL2 with a recent
kernel exposes cgroup v2, which DOMjudge ≥ 8.2 supports. If judging fails with
errors mentioning cgroups, enable systemd in `/etc/wsl.conf`:

```ini
[boot]
systemd=true
```

…then `wsl --shutdown` from PowerShell and start again.

## Tear down

```bash
docker compose down       # keeps volumes (DB + print log)
docker compose down -v    # wipes everything including the demo contest
```
