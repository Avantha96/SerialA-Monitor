/**
 * Tauri bridge.
 *
 * Installs the exact `window.electronAPI` surface the components already use,
 * so the React tree needed no rewrite when the backend moved to Rust.
 *
 * Performance detail: the Rust reader ships a batch of lines as ONE base64
 * blob plus a length table, at most every 8 ms. We register a single global
 * listener per event, decode the blob once, and hand each subscriber a
 * `subarray` view into that one buffer - no per-line JSON arrays, no copies.
 */
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

interface SerialConfigWire {
  path: string
  baudRate: number
  dataBits: number
  stopBits: number
  parity: string
  mode?: 'generic' | 'modbus'
}

interface DataEvent {
  id: string
  blob: string
  lens: number[]
}

// ── Subscriber registries ──────────────────────────────────────────────────
type DataCb = (id: string, data: Uint8Array) => void
type PairCb = (id: string, msg: string) => void
type IdCb = (id: string) => void

const dataSubs = new Set<DataCb>()
const errorSubs = new Set<PairCb>()
const closedSubs = new Set<IdCb>()
const reconnectingSubs = new Set<IdCb>()
const reconnectedSubs = new Set<IdCb>()

function decodeBlob(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

listen<DataEvent>('serial:data', ({ payload }) => {
  if (dataSubs.size === 0) return
  const bytes = decodeBlob(payload.blob)
  let off = 0
  for (const len of payload.lens) {
    // subarray shares the ArrayBuffer - one allocation per batch, not per line.
    const view = bytes.subarray(off, off + len)
    off += len
    for (const cb of dataSubs) cb(payload.id, view)
  }
})

listen<[string, string]>('serial:error', ({ payload }) => {
  for (const cb of errorSubs) cb(payload[0], payload[1])
})

listen<string>('serial:closed', ({ payload }) => {
  for (const cb of closedSubs) cb(payload)
})

listen<string>('serial:reconnecting', ({ payload }) => {
  for (const cb of reconnectingSubs) cb(payload)
})

listen<string>('serial:reconnected', ({ payload }) => {
  for (const cb of reconnectedSubs) cb(payload)
})

function subscribe<T>(set: Set<T>, cb: T): () => void {
  set.add(cb)
  return () => set.delete(cb)
}

// ── API surface ────────────────────────────────────────────────────────────
export const tauriAPI = {
  getPorts: () => invoke<Array<{ path: string; manufacturer: string }>>('get_ports'),

  connect: async (id: string, config: SerialConfigWire) => {
    try {
      return await invoke<boolean>('connect', { id, config })
    } catch (e) {
      return { error: String(e) }
    }
  },

  disconnect: (id: string) => invoke('disconnect', { id }),

  write: (id: string, data: string, isHex: boolean) =>
    invoke('write_data', { id, data, isHex }),

  setAutoReconnect: (id: string, enabled: boolean) =>
    invoke<void>('set_auto_reconnect', { id, enabled }),

  toggleLogger: async (id: string, enable: boolean) => {
    try {
      return await invoke<string | null>('toggle_logger', { id, enable })
    } catch {
      return null
    }
  },

  getStats: (id: string) =>
    invoke<{
      totalRx: number
      totalTx: number
      rxRate: number
      txRate: number
      rxChunks: number
      txChunks: number
      topBytes: Array<{ byte: number; count: number }>
    } | null>('get_stats', { id }),

  saveFile: (
    defaultFileName: string,
    content: string,
    filterName: string,
    extensions: string[]
  ) =>
    invoke<{ success: boolean; filePath?: string; error?: string }>('save_file', {
      defaultFileName,
      content,
      filterName,
      extensions,
    }),

  saveSession: (defaultFileName: string, content: string) =>
    tauriAPI.saveFile(defaultFileName, content, 'Text Documents', ['txt']),

  modbusRequest: (params: {
    tabId: string
    slaveId: number
    functionCode: number
    startAddress: number
    quantity: number
    writeValue: string
  }) => invoke<{ data?: number[]; error?: string }>('modbus_request', { params }),

  onData: (cb: DataCb) => subscribe(dataSubs, cb),
  onError: (cb: PairCb) => subscribe(errorSubs, cb),
  onClosed: (cb: IdCb) => subscribe(closedSubs, cb),
  onReconnecting: (cb: IdCb) => subscribe(reconnectingSubs, cb),
  onReconnected: (cb: IdCb) => subscribe(reconnectedSubs, cb),
}

// Keep the historical global name so components stay untouched.
window.electronAPI = tauriAPI as unknown as Window['electronAPI']
