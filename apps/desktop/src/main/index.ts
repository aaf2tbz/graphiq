import { app, BrowserWindow, dialog, ipcMain, nativeImage } from 'electron';
import fs from 'node:fs';
import path from 'node:path';
import {
  checkForUpdates,
  deleteIndex,
  getIndexDetails,
  indexProject,
  listConnectors,
  listIndexes,
  pairConnector,
  pullLatestMain,
  submitIssue,
  unpairConnector,
  uninstallApp
} from './graphiq-service';

let mainWindow: BrowserWindow | null = null;

async function createWindow() {
  const preloadCandidates = [
    path.join(__dirname, '../preload/index.mjs'),
    path.join(app.getAppPath(), 'out/preload/index.mjs'),
    path.join(process.cwd(), 'out/preload/index.mjs')
  ];
  const iconCandidates = [
    path.join(app.getAppPath(), 'build/icon.png'),
    path.join(process.cwd(), 'build/icon.png')
  ];
  const preloadPath = preloadCandidates.find((candidate) => fs.existsSync(candidate)) ?? preloadCandidates[0];
  const iconPath = iconCandidates.find((candidate) => fs.existsSync(candidate));
  if (iconPath && process.platform === 'darwin') {
    app.dock?.setIcon(nativeImage.createFromPath(iconPath));
  }
  mainWindow = new BrowserWindow({
    width: 1520,
    height: 980,
    minWidth: 1180,
    minHeight: 760,
    titleBarStyle: 'hiddenInset',
    backgroundColor: '#f3f4f2',
    icon: iconPath,
    webPreferences: {
      preload: preloadPath,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  });

  mainWindow.webContents.on('did-fail-load', (_event, code, description, url) => {
    console.error('[graphiq-desktop] load failure', { code, description, url });
  });

  if (process.env.ELECTRON_RENDERER_URL) {
    await mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    await mainWindow.loadFile(path.join(__dirname, '../renderer/index.html'));
  }
}

app.whenReady().then(async () => {
  ipcMain.handle('indexes:list', (_event, forceRefresh?: boolean) => listIndexes(forceRefresh));
  ipcMain.handle('indexes:details', (_event, projectPath: string) => getIndexDetails(projectPath));
  ipcMain.handle('indexes:create', (_event, projectPath: string) => indexProject(projectPath));
  ipcMain.handle('indexes:delete', (_event, projectPath: string) => deleteIndex(projectPath));
  ipcMain.handle('connectors:list', () => listConnectors());
  ipcMain.handle('connectors:pair', (_event, connectorId: string) => pairConnector(connectorId));
  ipcMain.handle('connectors:unpair', (_event, connectorId: string) => unpairConnector(connectorId));
  ipcMain.handle('dialog:choose-project', async () => {
    const result = await dialog.showOpenDialog({
      properties: ['openDirectory']
    });
    return result.canceled ? null : result.filePaths[0] ?? null;
  });
  ipcMain.handle('settings:check-updates', () => checkForUpdates());
  ipcMain.handle('settings:pull-latest', () => pullLatestMain());
  ipcMain.handle('settings:uninstall', () => uninstallApp());
  ipcMain.handle('settings:submit-issue', (_event, draft) => submitIssue(draft));

  await createWindow();

  app.on('activate', async () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      await createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});
