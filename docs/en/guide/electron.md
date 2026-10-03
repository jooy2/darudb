---
title: Electron
order: 12
languages: [node]
---

# Electron

The Node.js package runs in Electron's main process as it does in Node.js, with the same addon, since Node-API stays the same across Node.js and Electron versions.

## Where to open the database

Open the database in the main process, or in a utility process of your own (`utilityProcess`), and give the renderers what they need through IPC. A renderer is sandboxed by default and cannot load a native addon. The package's tests run it in Electron 44.

## Several instances of the app

Several processes can have the file open at once, two instances of the app included. Each opens it with a handle of its own, and the engine coordinates them through the operating system's file locks: one writes at a time, and a writer that waits longer than the busy timeout fails with `BUSY`. Within one process, opening the file again gives another handle to the same database. [Several processes](./processes.md) has the rules that come with the locks.

## Packaging

A packaged app should keep the addon outside the `asar` archive, from which Electron can load a native addon only by copying it out first. electron-builder leaves it outside by itself, and Electron Forge does with its auto-unpack-natives plugin.
