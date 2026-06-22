# Desktop App

A visual index browser, topology view, connector status, and project manager — an Electron shell that reads the same `.graphiq` indexes the CLI produces.

## Run it

```bash
cd apps/desktop
npm install
npm run dev      # development
```

## Build installers

```bash
cd apps/desktop
npm run package  # → apps/desktop/release/
```

## Session awareness

The desktop app follows the active Signet session workspace when Signet writes `$SIGNET_WORKSPACE/.daemon/graphiq/state.json`. Active session indexes are sorted first and marked in the project selector.
