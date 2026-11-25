import { SerialPort } from 'serialport'
import { BrowserWindow } from 'electron'
import { FileLogger } from './logger'

export interface SerialConfig {
  path: string
  baudRate: number
  dataBits: 8 | 7 | 6 | 5
  stopBits: 1 | 2
  parity: 'none' | 'even' | 'mark' | 'odd' | 'space'
}

export class SerialManager {
  private ports: Map<string, SerialPort> = new Map()
  private configs: Map<string, SerialConfig> = new Map()
  private reconnectTimers: Map<string, ReturnType<typeof setInterval>> = new Map()
  private reconnectEnabled: Set<string> = new Set()
  private explicitDisconnects: Set<string> = new Set()
  private lineBuffers: Map<string, Buffer> = new Map()
  private flushTimers: Map<string, NodeJS.Timeout> = new Map()
  private win: BrowserWindow
  private logger: FileLogger = new FileLogger()

  constructor(win: BrowserWindow) {
    this.win = win
  }

  public async getAvailablePorts() {
    try {
      const ports = await SerialPort.list()
      return ports.map(p => ({ path: p.path, manufacturer: p.manufacturer }))
    } catch (e) {
      console.error(e)
      return []
    }
  }

  public async connect(sessionId: string, config: SerialConfig): Promise<boolean> {
    if (this.ports.has(sessionId)) {
      this.disconnect(sessionId)
    }
    this.lineBuffers.set(sessionId, Buffer.alloc(0))
    const existingTimer = this.flushTimers.get(sessionId)
    if (existingTimer) clearTimeout(existingTimer)

    return new Promise((resolve, reject) => {
      try {
        const port = new SerialPort({
          path: config.path,
          baudRate: config.baudRate,
          dataBits: config.dataBits,
          stopBits: config.stopBits,
          parity: config.parity,
          autoOpen: false,
        })

        port.open((err) => {
          if (err) {
            reject(err.message)
            return
          }

          // Clear DTR and RTS explicitly to prevent boards like ESP32 from booting into download mode
          // or getting stuck in reset when the serial port is opened.
          port.set({ dtr: false, rts: false }, (setErr) => {
            if (setErr) console.error('Failed to clear DTR/RTS:', setErr)
          })

          port.on('data', (data: Buffer) => {
            const existing = this.lineBuffers.get(sessionId) || Buffer.alloc(0)
            this.lineBuffers.set(sessionId, Buffer.concat([existing, data]))

            const timer = this.flushTimers.get(sessionId)
            if (timer) clearTimeout(timer)

            this.processBuffer(sessionId, false)

            this.flushTimers.set(sessionId, setTimeout(() => {
              this.processBuffer(sessionId, true)
            }, 50))
          })

          port.on('error', (err) => {
            this.win.webContents.send('serial:error', sessionId, err.message)
          })

          port.on('close', () => {
            const timer = this.flushTimers.get(sessionId)
            if (timer) clearTimeout(timer)
            this.processBuffer(sessionId, true)
            this.lineBuffers.delete(sessionId)
            this.ports.delete(sessionId)
            this.win.webContents.send('serial:closed', sessionId)
            this.logger.stopLogging(sessionId)
            // Start reconnect polling if enabled and not a deliberate disconnect
            if (this.reconnectEnabled.has(sessionId) && !this.explicitDisconnects.has(sessionId)) {
              this.startReconnectPolling(sessionId)
            }
            this.explicitDisconnects.delete(sessionId)
          })

          this.ports.set(sessionId, port)
          this.configs.set(sessionId, config)
          resolve(true)
        })
      } catch (err: any) {
        reject(err.message)
      }
    })
  }

  public disconnect(sessionId: string) {
    this.explicitDisconnects.add(sessionId)  // Mark as intentional
    this.stopReconnectPolling(sessionId)
    const port = this.ports.get(sessionId)
    if (port && port.isOpen) port.close()
    
    const timer = this.flushTimers.get(sessionId)
    if (timer) clearTimeout(timer)
    this.flushTimers.delete(sessionId)
    
    this.ports.delete(sessionId)
    this.configs.delete(sessionId)
    this.lineBuffers.delete(sessionId)
  }

  private processBuffer(sessionId: string, forceFlush: boolean) {
    let buffer = this.lineBuffers.get(sessionId)
    if (!buffer || buffer.length === 0) return

    let offset = 0
    while (true) {
      const newLineIdx = buffer.indexOf(0x0A, offset)
      if (newLineIdx === -1) break

      let end = newLineIdx
      if (end > offset && buffer[end - 1] === 0x0D) {
        end--
      }
      
      const payload = buffer.subarray(offset, end)
      if (payload.length > 0) {
        this.win.webContents.send('serial:data', sessionId, new Uint8Array(payload))
        this.logger.logData(sessionId, new Uint8Array(payload), false)
      } else {
        this.win.webContents.send('serial:data', sessionId, new Uint8Array())
        this.logger.logData(sessionId, new Uint8Array(), false)
      }
      offset = newLineIdx + 1
    }

    if (forceFlush && offset < buffer.length) {
      const remainder = buffer.subarray(offset)
      this.win.webContents.send('serial:data', sessionId, new Uint8Array(remainder))
      this.logger.logData(sessionId, new Uint8Array(remainder), false)
      offset = buffer.length
    }

    this.lineBuffers.set(sessionId, buffer.subarray(offset))
  }

  public setAutoReconnect(sessionId: string, enabled: boolean) {
    if (enabled) {
      this.reconnectEnabled.add(sessionId)
    } else {
      this.reconnectEnabled.delete(sessionId)
      this.stopReconnectPolling(sessionId)
    }
  }

  private startReconnectPolling(sessionId: string) {
    if (this.reconnectTimers.has(sessionId)) return
    this.win.webContents.send('serial:reconnecting', sessionId)
    
    const timer = setInterval(async () => {
      const config = this.configs.get(sessionId)
      if (!config) { this.stopReconnectPolling(sessionId); return }
      
      try {
        const available = await SerialPort.list()
        const found = available.some(p => p.path === config.path)
        if (found) {
          this.stopReconnectPolling(sessionId)
          await this.connect(sessionId, config)
          this.win.webContents.send('serial:reconnected', sessionId)
        }
      } catch { /* keep trying */ }
    }, 2000)
    
    this.reconnectTimers.set(sessionId, timer)
  }

  private stopReconnectPolling(sessionId: string) {
    const timer = this.reconnectTimers.get(sessionId)
    if (timer) { clearInterval(timer); this.reconnectTimers.delete(sessionId) }
  }

  public getPortPath(sessionId: string): string | undefined {
    return this.configs.get(sessionId)?.path
  }

  public getBaudRate(sessionId: string): number | undefined {
    return this.configs.get(sessionId)?.baudRate
  }

  public write(sessionId: string, data: string, isHex: boolean) {
    const port = this.ports.get(sessionId)
    if (!port || !port.isOpen) return

    let writeBuffer: Buffer
    if (isHex) {
      // Remove spaces, prefix 0x, etc. and convert to Buffer
      const hexStr = data.replace(/\s+/g, '')
      writeBuffer = Buffer.from(hexStr, 'hex')
    } else {
      writeBuffer = Buffer.from(data, 'utf8')
    }

    port.write(writeBuffer, (err) => {
      if (err) {
        this.win.webContents.send('serial:error', sessionId, err.message)
      } else {
        // Log raw string send
        this.logger.logData(sessionId, new Uint8Array(writeBuffer), isHex)
      }
    })
  }

  public toggleLogger(sessionId: string, enable: boolean) {
    if (enable) {
      return this.logger.startLogging(sessionId)
    } else {
      this.logger.stopLogging(sessionId)
      return null
    }
  }

  public destroy() {
    this.logger.closeAll()
    for (const [_id, port] of this.ports.entries()) {
      if (port.isOpen) port.close()
    }
    for (const timer of this.reconnectTimers.values()) clearInterval(timer)
    this.reconnectTimers.clear()
    this.ports.clear()
  }
}
