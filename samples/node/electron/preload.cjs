/**
 * The bridge between the sandboxed window and the main process: one call
 * for every method of `core/protocol.ts`, and a way to hear a sample run's
 * progress. Nothing else of Electron or Node.js reaches the page.
 *
 * A sandboxed preload script loads as CommonJS, so this one file stays
 * JavaScript.
 */
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('sample', {
  call: (method, args) => ipcRenderer.invoke('sample:call', method, args),
  onProgress: (listener) => {
    const handler = (_event, progress) => listener(progress);

    ipcRenderer.on('sample:progress', handler);

    return () => {
      ipcRenderer.removeListener('sample:progress', handler);
    };
  }
});
