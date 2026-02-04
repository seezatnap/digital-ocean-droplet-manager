# DOCTL Droplet Manager TUI

A fast, keyboard-driven terminal UI for managing DigitalOcean droplets using `doctl`. Built in Rust with `ratatui`.

## Features
- List and filter droplets with status, region, size, IPs, and tags.
- Create droplets with guided selection (region, size, image, SSH keys, tags).
- Connect to a running droplet via `doctl compute ssh`.
- Snapshot + delete a droplet in a single safe workflow.
- Restore droplets from snapshots.
- Delete droplets without snapshot (explicit confirmation).
- Bind local ports to droplet ports with SSH tunnels and collision prevention.
- Sync local folders to droplets with Mutagen (persisted in `~/.mountlist` on the droplet).

## Requirements
- `doctl` installed and authenticated.
- SSH access to droplets (keys configured in DigitalOcean).
- Rust toolchain (for building/running).
- Mutagen installed for folder syncs (`brew install mutagen-io/mutagen/mutagen`).

## Run
```
cargo run
```

## Key Controls (Home)
- `g` refresh
- `c` create droplet
- `r` restore droplet from snapshot
- `s` snapshot + delete droplet
- `d` delete droplet (no snapshot)
- `b` bind local port to droplet port
- `m` sync local folders to droplet (Mutagen)
- `u` restore Mutagen syncs from `~/.mountlist`
- `y` list/delete Mutagen syncs
- `Enter` connect to selected droplet
- `p` manage port bindings
- `f` toggle running-only filter
- `q` quit

## Port Bindings
- Uses `ssh -N -L` to create local port forward tunnels.
- Prevents double-booking ports by checking a local registry and OS port availability.
- Active bindings are stored in a local JSON state file under your OS config directory.
- Stale bindings can be cleaned up from the bindings screen (`x`).

## Regions
- Regions are currently hardcoded in the app with availability flags.
- Update `src/doctl.rs` if you want to sync live from the API later.

## Tests
```
cargo test
```

## Security Notes
- No API tokens or secrets are stored by the app.
- The app uses your existing `doctl` configuration and context.

## Troubleshooting
- If lists appear empty, ensure `doctl` works in the same shell:
  - `doctl account get`
  - `doctl compute droplet list`
- If port binding fails, verify SSH user/key and accept the host key if prompted.
