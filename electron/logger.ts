import fs from 'node:fs'
import path from 'node:path'
import { app } from 'electron'

export class FileLogger {
  private loggers: Map<string, fs.WriteStream> = new Map()

  public startLogging(sessionId: string): string {
    if (this.loggers.has(sessionId)) return 'Already logging'
    
    // Save to user Documents folder
    const logDir = path.join(app.getPath('documents'), 'SerialMonitorLogs')
    if (!fs.existsSync(logDir)) {
      fs.mkdirSync(logDir, { recursive: true })
    }

    const timestamp = new Date().toISOString().replace(/[:.]/g, '-')
    const filename = `session_${sessionId}_${timestamp}.txt`
    const filepath = path.join(logDir, filename)

    const stream = fs.createWriteStream(filepath, { flags: 'a' })
    this.loggers.set(sessionId, stream)

    return filepath
  }

  public stopLogging(sessionId: string) {
    const stream = this.loggers.get(sessionId)
    if (stream) {
      stream.end()
      this.loggers.delete(sessionId)
    }
  }

  public logData(sessionId: string, data: Uint8Array, isHex: boolean = false) {
    const stream = this.loggers.get(sessionId)
    if (!stream) return

    const timestamp = new Date().toISOString()
    let text = ''

    if (isHex) {
      const hex = Buffer.from(data).toString('hex').toUpperCase().match(/.{1,2}/g)?.join(' ') || ''
      text = `[${timestamp}] [HEX] ${hex}\n`
    } else {
      text = `[${timestamp}] [ASC] ${Buffer.from(data).toString('utf8')}\n`
    }

    stream.write(text)
  }

  public closeAll() {
    for (const [_, stream] of this.loggers.entries()) {
      stream.end()
    }
    this.loggers.clear()
  }
}
