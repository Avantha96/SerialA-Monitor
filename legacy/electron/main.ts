import { app, BrowserWindow, ipcMain, dialog } from 'electron'
import path from 'node:path'
import fs from 'node:fs/promises'
import { SerialManager, SerialConfig } from './serialManager'
import ModbusRTU from 'modbus-serial'

process.env.DIST = path.join(__dirname, '../dist')

let win: BrowserWindow | null

const VITE_DEV_SERVER_URL = process.env['VITE_DEV_SERVER_URL']

function createWindow() {
  process.env.VITE_PUBLIC = app.isPackaged ? process.env.DIST : path.join(process.env.DIST || '', '../public')
  win = new BrowserWindow({
    icon: path.join(process.env.VITE_PUBLIC || '', 'electron-vite.svg'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
      contextIsolation: true,
    },
    width: 1200,
    height: 800,
  })

  // Test active push message to Renderer-process.
  win.webContents.on('did-finish-load', () => {
    win?.webContents.send('main-process-message', (new Date).toLocaleString())
  })

  if (VITE_DEV_SERVER_URL) {
    win.loadURL(VITE_DEV_SERVER_URL)
  } else {
    // win.loadFile('dist/index.html')
    win.loadFile(path.join(process.env.DIST || '', 'index.html'))
  }

  // Setup IPC and Serial Manager
  const serialManager = new SerialManager(win)

  ipcMain.handle('serial:get-ports', async () => {
    return await serialManager.getAvailablePorts()
  })

  ipcMain.handle('serial:connect', async (_event: any, id: string, config: SerialConfig) => {
    try {
      return await serialManager.connect(id, config)
    } catch (e: any) {
      return { error: e.message || e }
    }
  })

  ipcMain.handle('serial:disconnect', (_event: any, id: string) => {
    serialManager.disconnect(id)
  })

  ipcMain.handle('serial:set-auto-reconnect', (_event: any, id: string, enabled: boolean) => {
    serialManager.setAutoReconnect(id, enabled)
  })

  ipcMain.handle('serial:write', (_event: any, id: string, data: string, isHex: boolean) => {
    serialManager.write(id, data, isHex)
  })

  ipcMain.handle('serial:toggle-logger', (_event: any, id: string, enable: boolean) => {
    return serialManager.toggleLogger(id, enable)
  })

  ipcMain.handle('serial:save-session', async (_event: any, defaultFileName: string, content: string) => {
    if (!win) return { success: false, error: 'Window not initialized' }
    const { canceled, filePath } = await dialog.showSaveDialog(win, {
      title: 'Save Session Data',
      defaultPath: path.join(app.getPath('documents'), defaultFileName),
      filters: [{ name: 'Text Documents', extensions: ['txt'] }]
    })

    if (canceled || !filePath) return { success: false }

    try {
      await fs.writeFile(filePath, content, 'utf8')
      return { success: true, filePath }
    } catch (err: any) {
      return { success: false, error: err.message }
    }
  })

  ipcMain.handle('modbus:request', async (_event: any, params: {
    tabId: string, slaveId: number, functionCode: number,
    startAddress: number, quantity: number, writeValue: string
  }) => {
    const portPath = serialManager.getPortPath(params.tabId)
    if (!portPath) return { error: 'No open port for session' }

    const client = new ModbusRTU()
    try {
      await client.connectRTUBuffered(portPath, { baudRate: serialManager.getBaudRate(params.tabId) || 9600 })
      client.setID(params.slaveId)
      client.setTimeout(2000)

      let result: any
      const { functionCode: fc, startAddress, quantity, writeValue } = params

      if (fc === 1) {
        result = await client.readCoils(startAddress, quantity)
        return { data: result.data.map((b: boolean) => b ? 1 : 0) }
      } else if (fc === 2) {
        result = await client.readDiscreteInputs(startAddress, quantity)
        return { data: result.data.map((b: boolean) => b ? 1 : 0) }
      } else if (fc === 3) {
        result = await client.readHoldingRegisters(startAddress, quantity)
        return { data: result.data }
      } else if (fc === 4) {
        result = await client.readInputRegisters(startAddress, quantity)
        return { data: result.data }
      } else if (fc === 5) {
        await client.writeCoil(startAddress, parseInt(writeValue) !== 0)
        return { data: [parseInt(writeValue) !== 0 ? 1 : 0] }
      } else if (fc === 6) {
        const val = parseInt(writeValue)
        await client.writeRegister(startAddress, isNaN(val) ? 0 : val)
        return { data: [val] }
      } else if (fc === 15) {
        const vals = writeValue.split(',').map(s => parseInt(s.trim()) !== 0)
        await client.writeCoils(startAddress, vals)
        return { data: vals.map(b => b ? 1 : 0) }
      } else if (fc === 16) {
        const vals = writeValue.split(',').map(s => { const n = parseInt(s.trim()); return isNaN(n) ? 0 : n })
        await client.writeRegisters(startAddress, vals)
        return { data: vals }
      }

      return { error: `Function code ${fc} not supported` }
    } catch (e: any) {
      return { error: e.message || 'Modbus error' }
    } finally {
      try { client.close() } catch (_) {}
    }
  })

  app.on('before-quit', () => {
    serialManager.destroy()
  })
}

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit()
    win = null
  }
})

app.whenReady().then(createWindow)
