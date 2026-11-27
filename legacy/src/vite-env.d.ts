/// <reference types="vite/client" />

declare global {
  interface SerialConfig {
    path: string
    baudRate: number
    dataBits: 8 | 7 | 6 | 5
    stopBits: 1 | 2
    parity: 'none' | 'even' | 'mark' | 'odd' | 'space'
    /** Set at connect time so Rust knows to feed the console or the RTU parser. */
    mode?: 'generic' | 'modbus'
  }

  interface SessionStats {
    totalRx: number
    totalTx: number
    rxRate: number
    txRate: number
    rxChunks: number
    txChunks: number
    topBytes: Array<{ byte: number; count: number }>
  }

  interface SaveResult {
    success: boolean
    filePath?: string
    error?: string
  }

  /**
   * Backed by Rust/Tauri (see src/api.ts). The historical name is kept so the
   * component tree did not have to change when Electron was dropped.
   */
  interface ElectronAPI {
    getPorts: () => Promise<Array<{ path: string; manufacturer: string }>>
    connect: (id: string, config: SerialConfig) => Promise<boolean | { error: string }>
    disconnect: (id: string) => void
    write: (id: string, data: string, isHex: boolean) => void
    toggleLogger: (id: string, enable: boolean) => Promise<string | null>
    getStats: (id: string) => Promise<SessionStats | null>
    saveFile: (defaultFileName: string, content: string, filterName: string, extensions: string[]) => Promise<SaveResult>
    saveSession: (defaultFileName: string, content: string) => Promise<SaveResult>
    modbusRequest: (params: {
      tabId: string
      slaveId: number
      functionCode: number
      startAddress: number
      quantity: number
      writeValue: string
    }) => Promise<{ data?: number[]; error?: string }>
    setAutoReconnect: (id: string, enabled: boolean) => Promise<void>

    onData: (callback: (id: string, data: Uint8Array) => void) => () => void
    onError: (callback: (id: string, err: string) => void) => () => void
    onClosed: (callback: (id: string) => void) => () => void
    onReconnecting: (callback: (id: string) => void) => () => void
    onReconnected: (callback: (id: string) => void) => () => void
  }

  interface Window {
    electronAPI: ElectronAPI
  }
}

export {}
