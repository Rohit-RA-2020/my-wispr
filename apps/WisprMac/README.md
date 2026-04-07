## WisprMac

Menu bar app for macOS control of `wisprd`.

Run in development:

```bash
cd /Users/shivanshi/Documents/my-wispr
cargo build --bin wisprd --bin wisprctl
cd apps/WisprMac
swift run WisprMacApp
```

Default global shortcut:

- `Control + Option + Space`

The app will try to start `wisprd` automatically and can install a LaunchAgent from the settings window.
