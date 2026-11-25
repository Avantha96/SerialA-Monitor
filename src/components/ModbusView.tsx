import { useState, useEffect, useRef } from 'react'
import { Play, Square, RefreshCw, Download, Trash2 } from 'lucide-react'
import { useStore } from '../store'

type FunctionCode = 1 | 2 | 3 | 4 | 5 | 6 | 15 | 16

interface ModbusRegisterRow {
  address: number
  raw: number | null
  scaled: string
  status: 'ok' | 'error' | 'pending' | 'idle'
}

interface ModbusRequest {
  slaveId: number
  functionCode: FunctionCode
  startAddress: number
  quantity: number
  writeValue: string
  pollIntervalMs: number
}

const FC_LABELS: Record<FunctionCode, string> = {
  1: '01 – Read Coils',
  2: '02 – Read Discrete Inputs',
  3: '03 – Read Holding Registers',
  4: '04 – Read Input Registers',
  5: '05 – Write Single Coil',
  6: '06 – Write Single Register',
  15: '15 – Write Multiple Coils',
  16: '16 – Write Multiple Registers',
}

const WRITABLE: Set<FunctionCode> = new Set([5, 6, 15, 16])

function isWriteFC(fc: FunctionCode): boolean {
  return WRITABLE.has(fc)
}

export function ModbusView({ tabId }: { tabId: string }) {
  const addMessage = useStore(state => state.addMessage)
  const tab = useStore(state => state.tabs.find(t => t.id === tabId))

  const [req, setReq] = useState<ModbusRequest>({
    slaveId: 1,
    functionCode: 3,
    startAddress: 0,
    quantity: 10,
    writeValue: '0',
    pollIntervalMs: 1000,
  })

  const [rows, setRows] = useState<ModbusRegisterRow[]>([])
  const [isPolling, setIsPolling] = useState(false)
  const [lastPollTime, setLastPollTime] = useState<string | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const isConnected = tab?.isConnected || false

  // Build empty rows when quantity changes
  useEffect(() => {
    setRows(
      Array.from({ length: req.quantity }, (_, i) => ({
        address: req.startAddress + i,
        raw: null,
        scaled: '–',
        status: 'idle',
      }))
    )
  }, [req.startAddress, req.quantity])

  const stopPolling = () => {
    if (pollRef.current) {
      clearInterval(pollRef.current)
      pollRef.current = null
    }
    setIsPolling(false)
  }

  const executeRequest = async () => {
    if (!window.electronAPI || !isConnected) return

    // Mark all rows as pending
    setRows(prev => prev.map(r => ({ ...r, status: 'pending' })))
    setErrorMsg(null)

    try {
      const result = await window.electronAPI.modbusRequest({
        tabId,
        slaveId: req.slaveId,
        functionCode: req.functionCode,
        startAddress: req.startAddress,
        quantity: req.quantity,
        writeValue: req.writeValue,
      })

      const now = new Date().toLocaleTimeString([], { hour12: false })
      setLastPollTime(now)

      if (result.error) {
        setErrorMsg(result.error)
        setRows(prev => prev.map(r => ({ ...r, status: 'error' })))
        addMessage(tabId, {
          timestamp: new Date().toISOString(),
          payload: new TextEncoder().encode(`Modbus Error: ${result.error}`),
          isSent: false,
        })
        return
      }

      if (result.data && Array.isArray(result.data)) {
        setRows(
          result.data.map((val: number, i: number) => ({
            address: req.startAddress + i,
            raw: val,
            scaled: String(val),
            status: 'ok',
          }))
        )
      }
    } catch (e: any) {
      setErrorMsg(e.message || 'Unknown error')
      setRows(prev => prev.map(r => ({ ...r, status: 'error' })))
    }
  }

  const togglePolling = () => {
    if (isPolling) {
      stopPolling()
      return
    }

    setIsPolling(true)
    executeRequest()
    pollRef.current = setInterval(executeRequest, req.pollIntervalMs)
  }

  // Stop polling if disconnected
  useEffect(() => {
    if (!isConnected && isPolling) stopPolling()
  }, [isConnected])

  // Cleanup on unmount
  useEffect(() => { return () => stopPolling() }, [])

  const clearTable = () => {
    setRows(prev => prev.map(r => ({ ...r, raw: null, scaled: '–', status: 'idle' })))
    setLastPollTime(null)
    setErrorMsg(null)
  }

  const exportCsv = async () => {
    const header = 'Address,Value,Status\n'
    const body = rows.map(r => `${r.address},${r.raw ?? ''},${r.status}`).join('\n')
    // Webview-initiated downloads are sandboxed; go through the native save
    // dialog in Rust instead.
    const res = await window.electronAPI.saveFile(
      `modbus_${req.slaveId}_${req.startAddress}.csv`,
      header + body,
      'CSV',
      ['csv']
    )
    if (res.error) setErrorMsg(`Export failed: ${res.error}`)
  }

  const updateReq = (key: keyof ModbusRequest, value: any) => {
    setReq(prev => ({ ...prev, [key]: value }))
    if (isPolling) stopPolling()
  }

  const labelClass = 'text-xs text-gray-400 font-semibold uppercase tracking-wider block mb-1'
  const inputClass = 'bg-gray-800 border border-gray-600 text-white text-sm rounded-md px-3 py-2 focus:outline-none focus:border-purple-500 focus:ring-1 focus:ring-purple-500/50 w-full transition-colors'

  return (
    <div className="flex flex-col h-full w-full bg-gray-950 overflow-hidden">
      {/* Modbus Control Bar */}
      <div className="bg-gray-900 border-b border-gray-800 px-5 py-4 flex flex-wrap gap-4 items-end shrink-0">
        {/* Slave ID */}
        <div className="flex flex-col w-20">
          <label className={labelClass}>Slave ID</label>
          <input type="number" min={1} max={247} className={inputClass}
            value={req.slaveId}
            onChange={e => updateReq('slaveId', parseInt(e.target.value) || 1)}
          />
        </div>

        {/* Function Code */}
        <div className="flex flex-col w-60">
          <label className={labelClass}>Function Code</label>
          <select className={inputClass}
            value={req.functionCode}
            onChange={e => updateReq('functionCode', parseInt(e.target.value) as FunctionCode)}
          >
            {(Object.keys(FC_LABELS) as unknown as FunctionCode[]).map(fc => (
              <option key={fc} value={fc}>{FC_LABELS[fc]}</option>
            ))}
          </select>
        </div>

        {/* Start Address */}
        <div className="flex flex-col w-28">
          <label className={labelClass}>Start Addr</label>
          <input type="number" min={0} max={65535} className={inputClass}
            value={req.startAddress}
            onChange={e => updateReq('startAddress', parseInt(e.target.value) || 0)}
          />
        </div>

        {/* Quantity (only for read FCs) */}
        {!isWriteFC(req.functionCode) && (
          <div className="flex flex-col w-20">
            <label className={labelClass}>Quantity</label>
            <input type="number" min={1} max={125} className={inputClass}
              value={req.quantity}
              onChange={e => updateReq('quantity', parseInt(e.target.value) || 1)}
            />
          </div>
        )}

        {/* Write Value (for write FCs) */}
        {isWriteFC(req.functionCode) && (
          <div className="flex flex-col w-40">
            <label className={labelClass}>Write Value</label>
            <input type="text" className={inputClass}
              placeholder={req.functionCode === 5 ? '0 or 1' : '0–65535'}
              value={req.writeValue}
              onChange={e => updateReq('writeValue', e.target.value)}
            />
          </div>
        )}

        {/* Poll Interval (only for read FCs) */}
        {!isWriteFC(req.functionCode) && (
          <div className="flex flex-col w-28">
            <label className={labelClass}>Poll Interval</label>
            <div className="flex items-center">
              <input type="number" min={50} step={50} className={inputClass}
                value={req.pollIntervalMs}
                onChange={e => updateReq('pollIntervalMs', parseInt(e.target.value) || 1000)}
              />
              <span className="ml-2 text-xs text-gray-500 shrink-0">ms</span>
            </div>
          </div>
        )}

        {/* Action Buttons */}
        <div className="flex items-center space-x-2 ml-auto">
          {!isWriteFC(req.functionCode) ? (
            <>
              <button
                disabled={!isConnected}
                onClick={togglePolling}
                className={`flex items-center space-x-2 px-4 py-2 rounded-md font-medium text-sm transition-all disabled:opacity-50 ${
                  isPolling
                    ? 'bg-red-500/10 text-red-400 border border-red-500/30 hover:bg-red-500/20'
                    : 'bg-purple-600 hover:bg-purple-500 text-white shadow-[0_0_15px_rgba(147,51,234,0.4)]'
                }`}
              >
                {isPolling ? <Square size={14} className="mr-1" /> : <Play size={14} className="mr-1" />}
                {isPolling ? 'Stop' : 'Start Polling'}
              </button>
              <button
                disabled={!isConnected || isPolling}
                onClick={executeRequest}
                className="p-2 rounded-md bg-gray-800 border border-gray-700 text-gray-300 hover:text-white hover:bg-gray-700 transition-colors disabled:opacity-40"
                title="Single Read"
              >
                <RefreshCw size={16} />
              </button>
            </>
          ) : (
            <button
              disabled={!isConnected}
              onClick={executeRequest}
              className="flex items-center px-4 py-2 rounded-md bg-amber-500/10 text-amber-400 border border-amber-500/30 hover:bg-amber-500/20 font-medium text-sm transition-all disabled:opacity-50"
            >
              <Play size={14} className="mr-1.5" /> Write
            </button>
          )}

          <button
            onClick={exportCsv}
            disabled={rows.every(r => r.raw === null)}
            className="p-2 rounded-md bg-gray-800 border border-gray-700 text-gray-300 hover:text-white hover:bg-gray-700 transition-colors disabled:opacity-40"
            title="Export CSV"
          >
            <Download size={16} />
          </button>
          <button onClick={clearTable} className="p-2 rounded-md bg-gray-800 border border-gray-700 text-gray-300 hover:text-red-400 hover:bg-gray-700 transition-colors" title="Clear">
            <Trash2 size={16} />
          </button>
        </div>
      </div>

      {/* Status Bar */}
      {(errorMsg || lastPollTime) && (
        <div className={`px-5 py-1.5 text-xs font-mono shrink-0 flex items-center space-x-4 ${errorMsg ? 'bg-red-900/20 border-b border-red-800/40 text-red-400' : 'bg-gray-900 border-b border-gray-800 text-gray-400'}`}>
          {errorMsg && <span>⚠ {errorMsg}</span>}
          {lastPollTime && !errorMsg && <span>✓ Last poll at {lastPollTime}</span>}
          {isPolling && <span className="animate-pulse text-purple-400 ml-auto">● Polling every {req.pollIntervalMs}ms</span>}
        </div>
      )}

      {/* Register Table */}
      <div className="flex-1 overflow-auto p-4">
        <table className="w-full text-sm font-mono border-collapse">
          <thead>
            <tr className="text-left text-xs text-gray-500 uppercase tracking-wider border-b border-gray-800">
              <th className="px-4 py-2 w-24">Address</th>
              <th className="px-4 py-2 w-24">Hex</th>
              <th className="px-4 py-2 w-32">Dec</th>
              <th className="px-4 py-2 flex-1">Binary</th>
              <th className="px-4 py-2 w-20 text-right">Status</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(row => {
              const statusColors: Record<string, string> = {
                ok: 'text-emerald-400',
                error: 'text-red-400',
                pending: 'text-yellow-400 animate-pulse',
                idle: 'text-gray-600',
              }
              const rowBg = row.status === 'error' ? 'bg-red-900/10' : row.status === 'pending' ? 'bg-yellow-900/5' : ''

              return (
                <tr
                  key={row.address}
                  className={`border-b border-gray-800/50 hover:bg-gray-800/40 transition-colors ${rowBg}`}
                >
                  <td className="px-4 py-2 text-gray-300">
                    <span className="text-gray-500 mr-1">%</span>
                    {req.startAddress + (row.address - req.startAddress)}
                    <span className="text-gray-600 text-xs ml-1">(0x{row.address.toString(16).padStart(4, '0').toUpperCase()})</span>
                  </td>
                  <td className="px-4 py-2 text-blue-300">
                    {row.raw !== null ? `0x${row.raw.toString(16).padStart(4, '0').toUpperCase()}` : '–'}
                  </td>
                  <td className="px-4 py-2 text-white">
                    {row.raw !== null ? row.raw : '–'}
                  </td>
                  <td className="px-4 py-2 text-gray-400 text-xs tracking-wider">
                    {row.raw !== null ? row.raw.toString(2).padStart(16, '0').replace(/(.{4})/g, '$1 ').trim() : '–'}
                  </td>
                  <td className={`px-4 py-2 text-right text-xs font-bold uppercase ${statusColors[row.status]}`}>
                    {row.status}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>

        {rows.length === 0 && (
          <div className="flex items-center justify-center h-48 text-gray-600 text-sm">
            Configure the request and click "Start Polling" or "Single Read"
          </div>
        )}
      </div>
    </div>
  )
}
