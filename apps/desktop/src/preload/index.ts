import { contextBridge, ipcRenderer } from 'electron';
import type { DesktopApi, IssueDraft } from '@shared/types';

const api: DesktopApi = {
  listIndexes: (forceRefresh = false) => ipcRenderer.invoke('indexes:list', forceRefresh),
  getIndexDetails: (projectPath) => ipcRenderer.invoke('indexes:details', projectPath),
  indexProject: (projectPath) => ipcRenderer.invoke('indexes:create', projectPath),
  deleteIndex: (projectPath) => ipcRenderer.invoke('indexes:delete', projectPath),
  listConnectors: () => ipcRenderer.invoke('connectors:list'),
  pairConnector: (connectorId) => ipcRenderer.invoke('connectors:pair', connectorId),
  unpairConnector: (connectorId) => ipcRenderer.invoke('connectors:unpair', connectorId),
  chooseProjectFolder: () => ipcRenderer.invoke('dialog:choose-project'),
  checkForUpdates: () => ipcRenderer.invoke('settings:check-updates'),
  pullLatestMain: () => ipcRenderer.invoke('settings:pull-latest'),
  uninstallApp: () => ipcRenderer.invoke('settings:uninstall'),
  submitIssue: (draft: IssueDraft) => ipcRenderer.invoke('settings:submit-issue', draft)
};

contextBridge.exposeInMainWorld('graphiq', api);
