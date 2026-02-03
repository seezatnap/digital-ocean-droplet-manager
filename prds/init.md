# PRD: doctl Control TUI (Rust + ratatui)

## Summary
Build a terminal UI that controls DigitalOcean droplets via `doctl`. The TUI must support: create droplet, connect to a running droplet, snapshot-and-delete a running droplet, restore a droplet from snapshot, and bind localhost ports to a droplet's exposed ports with strict local port management to prevent double-booking.

## Problem
Managing droplets via CLI is powerful but requires memorizing commands, juggling IDs, and manually managing SSH tunnels and local port assignments. This adds friction for common operations and increases the risk of mistakes (wrong droplet, port collisions, forgotten tunnels).

## Goals
- Provide a fast, keyboard-driven TUI for core droplet lifecycle workflows.
- Minimize mistakes with confirmations, previews, and safe defaults.
- Provide reliable local port binding with collision prevention and cleanup.
- Operate entirely through `doctl` to avoid embedding secrets or re-implementing the API.

## Non-Goals
- Full coverage of all DigitalOcean resources.
- Advanced cloud-init configuration or provisioning automation.
- GUI, web UI, or background daemon.
- Multi-account orchestration beyond what `doctl` contexts already support.

## Target Users
- Developers who regularly create, connect to, snapshot, and delete droplets.
- Users who need SSH tunnels to services running on droplets without port collisions.

## Assumptions
- `doctl` is installed and authenticated.
- SSH keys are configured and usable by `doctl compute ssh`.
- The TUI runs on macOS or Linux with a standard terminal.

## Primary User Stories
1. As a user, I can list droplets with status and key metadata.
2. As a user, I can create a droplet with common parameters from a guided form.
3. As a user, I can connect to a running droplet from the TUI.
4. As a user, I can snapshot a running droplet and then delete it safely.
5. As a user, I can restore a new droplet from a snapshot.
6. As a user, I can bind a local port to a droplet port and avoid collisions.
7. As a user, I can see and manage active local port bindings.

## Functional Requirements
### Droplet Listing
- Show droplet name, ID, status, region, size, and public IP.
- Provide filtering by status (e.g., running, off).
- Provide a refresh action that re-queries `doctl`.

### Create Droplet
- Guided form with required fields: name, region, size, image, SSH keys.
- Optional fields: tags, VPC, project.
- Confirm screen showing final `doctl compute droplet create` command parameters.
- Show progress and final droplet info on success.

### Connect to Droplet
- Action available only when droplet is running and has a public IP.
- Default action uses `doctl compute ssh`.
- The TUI should suspend, run the SSH session, and then resume on exit.

### Snapshot and Delete Droplet
- Action available only when droplet is running.
- Workflow: confirm snapshot name -> create snapshot -> confirm delete -> delete droplet.
- Ensure snapshot creation completes successfully before deletion.
- Show errors if snapshot fails or times out.

### Restore Droplet from Snapshot
- List available snapshots (droplet snapshots only).
- Guided form to set new droplet name, region, size, and SSH keys.
- Create droplet from selected snapshot.

### Local Port Binding (SSH Tunnels)
- Bind local port to a droplet port using SSH local forwarding.
- Require droplet running and public IP present.
- Capture bindings in a local registry so ports are not double-booked.
- Enforce that a port cannot be bound if:
  - Already registered in the local registry.
  - Already in use on the OS (best-effort check).
- Provide list of active bindings with ability to unbind.
- On TUI exit, attempt to gracefully terminate tunnel processes.

## Port Management Rules
- Port registry is persisted to disk (local user config dir).
- Each binding record includes: droplet ID, droplet name, remote port, local port, created timestamp, and tunnel process PID.
- On app startup, load registry and validate each entry:
  - If PID is not running, mark as stale and allow cleanup.
  - If local port is no longer bound by the tunnel, allow cleanup.
- A port cannot be assigned to more than one droplet at a time.

## UX Requirements
- Ratatui-based UI with keyboard navigation.
- Global actions: refresh, search, quit, help.
- Confirmation dialogs for destructive actions.
- Clear error messages with remediation hints.
- Non-blocking status indicators for long-running operations.

## Technical Requirements
- Language: Rust.
- UI: ratatui + crossterm (or equivalent terminal backend).
- Command execution via `doctl` and `ssh`, captured with stdout/stderr handling.
- No storage of API tokens in the app; rely on `doctl` config.
- Local state stored in a simple JSON or TOML file in user config directory.

## Data Model (Local State)
- PortBinding
  - droplet_id: string
  - droplet_name: string
  - local_port: u16
  - remote_port: u16
  - created_at: RFC3339 timestamp
  - tunnel_pid: optional integer

## Error Handling
- Fail fast on missing `doctl` or unauthenticated context with clear instructions.
- Surface `doctl` errors directly in the UI with minimal parsing.
- If snapshot creation is slow, show progress and allow cancelation.
- If tunnel creation fails, do not write registry entry.

## Security and Safety
- Confirmations for delete and snapshot actions.
- No plaintext secrets stored.
- Avoid command injection by validating user input and using structured args.

## Out of Scope for V1
- Autoscaling, load balancers, or firewall rules.
- Multiple concurrent `doctl` contexts management inside the app.
- DNS and domain management.

## Open Questions
- Preferred defaults for region, size, and image.
- Should port bindings allow specifying remote host other than localhost on the droplet?
- Should the app support automatic cleanup of stale tunnel records on startup?

## Success Metrics
- Time to create and connect to a droplet is under 60 seconds.
- Zero port collisions during binding operations in typical usage.
- Users can restore a snapshot without leaving the TUI.
