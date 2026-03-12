# macOS launchd

Use these scripts to run MapFlow as a per-user `launchd` service on macOS.

## Files

- `launchd-install.sh`: install/update and start LaunchAgent
- `launchd-uninstall.sh`: stop and remove LaunchAgent
- `launchd-status.sh`: inspect service status, health, and logs

## Quick Start

```bash
# 1) Install service (replace with your absolute binary path)
bash scripts/macos/launchd-install.sh \
  --binary /absolute/path/to/mapflow \
  --listen 127.0.0.1:3000 \
  --force

# 2) Check service
bash scripts/macos/launchd-status.sh --listen 127.0.0.1:3000
```

## Defaults

- Label: `io.mapflow.server`
- LaunchAgent plist: `~/Library/LaunchAgents/io.mapflow.server.plist`
- DB: `~/.mapflow/data/mapflow.duckdb`
- Uploads: `~/.mapflow/uploads`
- Logs:
  - `~/.mapflow/logs/server.stdout.log`
  - `~/.mapflow/logs/server.stderr.log`

## Uninstall

```bash
bash scripts/macos/launchd-uninstall.sh
```

## Notes

- This is a user-level service (`gui/$UID/...`), not a system daemon.
- To keep existing data/logs, uninstall only removes the LaunchAgent plist.
