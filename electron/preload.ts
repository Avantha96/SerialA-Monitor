import { ipcRenderer, contextBridge } from 'electron'

export interface SerialConfig {
  path: string
  baudRate: number
  dataBits: 8 | 7 | 6 | 5
  stopBits: 1 | 2
  parity: 'none' | 'even' | 'mark' | 'odd' | 'space'
}

export const electronAPI = {
  getPorts: () => ipcRenderer.invoke('serial:get-ports'),
  connect: (id: string, config: SerialConfig) => ipcRenderer.invoke('serial:connect', id, config),
  disconnect: (id: string) => ipcRenderer.invoke('serial:disconnect', id),
  write: (id: string, data: string, isHex: boolean) => ipcRenderer.invoke('serial:write', id, data, isHex),
  
  toggleLogger: (id: string, enable: boolean) => ipcRenderer.invoke('serial:toggle-logger', id, enable),
  saveSession: (defaultFileName: string, content: string) => ipcRenderer.invoke('serial:save-session', defaultFileName, content),
  modbusRequest: (params: { tabId: string, slaveId: number, functionCode: number, startAddress: number, quantity: number, writeValue: string }) =>
    ipcRenderer.invoke('modbus:request', params),
  setAutoReconnect: (id: string, enabled: boolean) => ipcRenderer.invoke('serial:set-auto-reconnect', id, enabled),
  
  // Events
  onData: (callback: (id: string, data: Uint8Array) => void) => {
    const listener = (_event: any, id: string, data: Uint8Array) => callback(id, data)
    ipcRenderer.on('serial:data', listener)
    return () => ipcRenderer.off('serial:data', listener)
  },
  onError: (callback: (id: string, err: string) => void) => {
    const listener = (_event: any, id: string, err: string) => callback(id, err)
    ipcRenderer.on('serial:error', listener)
    return () => ipcRenderer.off('serial:error', listener)
  },
  onClosed: (callback: (id: string) => void) => {
    const listener = (_event: any, id: string) => callback(id)
    ipcRenderer.on('serial:closed', listener)
    return () => ipcRenderer.off('serial:closed', listener)
  },
  onReconnecting: (callback: (id: string) => void) => {
    const listener = (_event: any, id: string) => callback(id)
    ipcRenderer.on('serial:reconnecting', listener)
    return () => ipcRenderer.off('serial:reconnecting', listener)
  },
  onReconnected: (callback: (id: string) => void) => {
    const listener = (_event: any, id: string) => callback(id)
    ipcRenderer.on('serial:reconnected', listener)
    return () => ipcRenderer.off('serial:reconnected', listener)
  }
}

contextBridge.exposeInMainWorld('electronAPI', electronAPI)
