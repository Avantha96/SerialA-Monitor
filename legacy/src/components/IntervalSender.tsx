import { useState, useEffect, useRef } from 'react'
import { useStore } from '../store'
import { Send, Clock, StopCircle, RefreshCw } from 'lucide-react'

type LineEnding = 'none' | 'nl' | 'cr' | 'nlcr'

const LINE_ENDING_LABELS: Record<LineEnding, string> = {
  none: 'No line ending',
  nl: 'New line',
  cr: 'Carriage return',
  nlcr: 'Both NL & CR',
}

const LINE_ENDING_BYTES: Record<LineEnding, string> = {
  none: '',
  nl: '\n',
  cr: '\r',
  nlcr: '\r\n',
}

export function IntervalSender({ tabId }: { tabId: string }) {
  const tab = useStore(state => state.tabs.find(t => t.id === tabId))
  const updateTab = useStore(state => state.updateTab)
  const addMessage = useStore(state => state.addMessage)

  const [inputVal, setInputVal] = useState('')
  const [delay, setDelay] = useState(1000)
  const [lineEnding, setLineEnding] = useState<LineEnding>('nl')
  const [history, setHistory] = useState<string[]>([])
  const [historyIdx, setHistoryIdx] = useState(-1)
  const inputRef = useRef<HTMLInputElement>(null)

  const buildPayloadStr = (val: string): string => {
    const suffix = LINE_ENDING_BYTES[lineEnding]
    return val + suffix
  }

  // No Node Buffer in a Tauri webview - parse hex ourselves.
  const hexToBytes = (val: string): Uint8Array => {
    const digits = val.replace(/[^0-9a-fA-F]/g, '')
    const len = digits.length >> 1
    const out = new Uint8Array(len)
    for (let i = 0; i < len; i++) out[i] = parseInt(digits.slice(i * 2, i * 2 + 2), 16)
    return out
  }

  const buildUint8 = (val: string): Uint8Array => {
    if (tab?.displayMode === 'hex') return hexToBytes(val)
    return new TextEncoder().encode(buildPayloadStr(val))
  }

  const handleSend = () => {
    if (!window.electronAPI || !tab?.isConnected || !inputVal.trim()) return
    const isHex = tab.displayMode === 'hex'
    const strToSend = isHex ? inputVal : buildPayloadStr(inputVal)
    window.electronAPI.write(tab.id, strToSend, isHex)
    addMessage(tab.id, {
      timestamp: new Date().toISOString(),
      isSent: true,
      payload: buildUint8(inputVal),
    })

    // Push to history (avoid duplicates at top)
    setHistory(prev => {
      const filtered = prev.filter(h => h !== inputVal)
      return [inputVal, ...filtered].slice(0, 50)
    })
    setHistoryIdx(-1)
    setInputVal('')
  }

  // PgUp / PgDown history navigation
  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleSend()
      return
    }

    const goBack = e.key === 'PageUp' || (e.key === 'ArrowUp' && e.ctrlKey)
    const goForward = e.key === 'PageDown' || (e.key === 'ArrowDown' && e.ctrlKey)

    if (goBack && history.length > 0) {
      e.preventDefault()
      setHistoryIdx(prev => {
        const next = Math.min(prev + 1, history.length - 1)
        setInputVal(history[next] ?? '')
        return next
      })
    } else if (goForward) {
      e.preventDefault()
      setHistoryIdx(prev => {
        const next = prev - 1
        if (next < 0) { setInputVal(''); return -1 }
        setInputVal(history[next] ?? '')
        return next
      })
    }
  }

  // Interval send effect
  useEffect(() => {
    if (!tab?.intervalActive || !tab?.isConnected || !tab?.intervalData) return
    const isHex = tab.displayMode === 'hex'
    const strToSend = isHex ? tab.intervalData : buildPayloadStr(tab.intervalData)

    const interval = setInterval(() => {
      window.electronAPI.write(tab.id, strToSend, isHex)
      addMessage(tab.id, {
        timestamp: new Date().toISOString(),
        isSent: true,
        payload: buildUint8(tab.intervalData),
      })
    }, tab.intervalDelayMs)

    return () => clearInterval(interval)
  }, [tab?.intervalActive, tab?.isConnected, tab?.intervalData, tab?.intervalDelayMs, tab?.displayMode, lineEnding])

  if (!tab) return null

  const toggleInterval = () => {
    updateTab(tab.id, {
      intervalActive: !tab.intervalActive,
      intervalData: inputVal || tab.intervalData,
      intervalDelayMs: delay,
    })
  }

  return (
    <div className="bg-gray-800 border-t border-gray-700 px-3 py-2 flex flex-wrap gap-2 items-center shadow-lg relative z-20">

      {/* Input */}
      <div className="flex-1 min-w-[180px] relative">
        <input
          ref={inputRef}
          type="text"
          value={inputVal}
          onChange={e => { setInputVal(e.target.value); setHistoryIdx(-1) }}
          onKeyDown={handleKeyDown}
          placeholder={
            tab.displayMode === 'hex'
              ? 'Hex bytes (e.g. 0A FF 10) — PgUp/Down for history'
              : 'Send data… PgUp/Down or Ctrl+↑↓ for history'
          }
          className="w-full bg-gray-900 text-green-400 font-mono text-sm rounded-md pl-3 pr-10 py-2 border border-gray-700 focus:outline-none focus:border-blue-500 placeholder-gray-600"
        />
        <button
          onClick={handleSend}
          disabled={!tab.isConnected || !inputVal.trim()}
          className="absolute right-2 top-1.5 p-1 text-gray-400 hover:text-blue-400 disabled:opacity-50 disabled:hover:text-gray-400 transition-colors"
        >
          <Send size={16} />
        </button>
      </div>

      {/* Line Ending Selector */}
      <select
        value={lineEnding}
        onChange={e => setLineEnding(e.target.value as LineEnding)}
        className="bg-gray-900 border border-gray-700 text-gray-300 text-xs rounded-md px-2 py-2 focus:outline-none focus:border-blue-500 transition-colors"
        title="Line ending"
        disabled={tab.displayMode === 'hex'}
      >
        {(Object.keys(LINE_ENDING_LABELS) as LineEnding[]).map(le => (
          <option key={le} value={le}>{LINE_ENDING_LABELS[le]}</option>
        ))}
      </select>

      <div className="h-6 w-px bg-gray-700 shrink-0" />

      {/* Interval Setup */}
      <div className="flex items-center space-x-2 bg-gray-900 rounded-md border border-gray-700 py-1 pl-3 pr-1">
        <Clock size={14} className="text-gray-500" />
        <input
          type="number"
          value={delay}
          onChange={e => setDelay(parseInt(e.target.value) || 1000)}
          className="w-16 bg-transparent text-sm text-gray-300 focus:outline-none text-right font-mono"
          min={10}
        />
        <span className="text-xs text-gray-500 pr-2">ms</span>
        <button
          onClick={toggleInterval}
          disabled={!tab.isConnected || (!inputVal.trim() && !tab.intervalData)}
          className={`px-3 py-1.5 rounded text-sm font-medium flex items-center transition-all disabled:opacity-50 ${
            tab.intervalActive
              ? 'bg-red-500/20 text-red-400 hover:bg-red-500/30'
              : 'bg-blue-600 hover:bg-blue-500 text-white shadow-md'
          }`}
        >
          {tab.intervalActive ? (
            <><StopCircle size={14} className="mr-1" /> Stop</>
          ) : (
            <><RefreshCw size={14} className="mr-1" /> Interval</>
          )}
        </button>
      </div>

      {/* History hint */}
      {history.length > 0 && (
        <span className="text-xs text-gray-600 select-none hidden sm:inline">
          {historyIdx >= 0 ? `${historyIdx + 1}/${history.length}` : `${history.length} in history`}
        </span>
      )}
    </div>
  )
}
