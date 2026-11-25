import { memo, useRef, useState, useMemo, useCallback, useEffect } from 'react'
import { VirtuosoHandle, Virtuoso } from 'react-virtuoso'
import { useStore } from '../store'
import { ArrowDownToLine, BarChart2, TrendingUp, Regex, X } from 'lucide-react'
import { StatsPanel } from './StatsPanel'
import { PlotterView } from './PlotterView'

// ── Formatting helpers ────────────────────────────────────────────────────
const formatHex = (buf: Uint8Array) =>
  Array.from(buf).map(b => b.toString(16).padStart(2, '0').toUpperCase()).join(' ')

const formatAscii = (buf: Uint8Array) => new TextDecoder().decode(buf)

// Highlight framing bytes in a hex string
function highlightFraming(hexStr: string, startByte: string, endByte: string) {
  const start = startByte.toUpperCase()
  const end = endByte.toUpperCase()
  const parts = hexStr.split(' ')
  return parts.map((byte, i) => {
    if (byte === start)
      return <span key={i} className="bg-emerald-500/25 text-emerald-300 rounded px-0.5">{byte} </span>
    if (byte === end)
      return <span key={i} className="bg-red-500/25 text-red-300 rounded px-0.5">{byte} </span>
    return <span key={i}>{byte} </span>
  })
}

// ── Console Row ────────────────────────────────────────────────────────────
interface RowProps {
  msg: any
  displayMode: 'ascii' | 'hex' | 'both'
  deltaMs: number | null
  fontSize: number
  framingEnabled: boolean
  framingStart: string
  framingEnd: string
}

function ConsoleRow({ msg, displayMode, deltaMs, fontSize, framingEnabled, framingStart, framingEnd }: RowProps) {
  const rawDate = new Date(msg.timestamp)
  const time = rawDate.toLocaleTimeString([], { hour12: false }) + '.' + rawDate.getMilliseconds().toString().padStart(3, '0')
  const dirColor = msg.isSent ? 'text-blue-400' : 'text-emerald-400'
  const label = msg.isSent ? 'TX:' : 'RX:'
  const style = { fontSize }

  const hexStr = formatHex(msg.payload)
  const hexContent = (framingEnabled && !msg.isSent)
    ? <>{highlightFraming(hexStr, framingStart, framingEnd)}</>
    : <>{hexStr}</>

  if (displayMode === 'both') {
    const asciiStr = formatAscii(msg.payload)
    return (
      <div className={`w-full px-4 py-1.5 font-mono border-b border-gray-800 hover:bg-gray-800/50 transition-colors ${dirColor}`} style={style}>
        <div className="flex">
          <span className="text-gray-500 w-28 shrink-0 select-none">[{time}]</span>
          {deltaMs !== null && <span className="text-yellow-600 w-16 shrink-0 text-xs self-center select-none">+{deltaMs}ms</span>}
          <span className="font-bold w-10 shrink-0 select-none">{label}</span>
          <span className="text-emerald-300 flex-1 break-words whitespace-pre-wrap">
            <span className="text-gray-500 text-xs mr-1">ASC</span>{asciiStr}
          </span>
        </div>
        <div className="flex">
          <span className="w-28 shrink-0" />
          {deltaMs !== null && <span className="w-16 shrink-0" />}
          <span className="w-10 shrink-0" />
          <span className="text-blue-300 flex-1 break-words whitespace-pre-wrap">
            <span className="text-gray-500 text-xs mr-2">HEX</span>{hexContent}
          </span>
        </div>
      </div>
    )
  }

  const content = displayMode === 'hex' ? hexContent : formatAscii(msg.payload)
  return (
    <div className={`flex w-full px-4 py-1 font-mono border-b border-gray-800 hover:bg-gray-800/50 transition-colors ${dirColor}`} style={style}>
      <span className="text-gray-500 w-28 shrink-0 select-none">[{time}]</span>
      {deltaMs !== null && <span className="text-yellow-600 w-16 shrink-0 text-xs self-center select-none">+{deltaMs}ms</span>}
      <span className="font-bold w-10 shrink-0 select-none">{label}</span>
      <span className="break-words whitespace-pre-wrap flex-1">{content}</span>
    </div>
  )
}

const MemoizedRow = memo(ConsoleRow)

// ── Main ConsoleView ────────────────────────────────────────────────────────
export function ConsoleView({ tabId }: { tabId: string }) {
  const tab = useStore(state => state.tabs.find(t => t.id === tabId))
  const updateTab = useStore(state => state.updateTab)
  const clearMessages = useStore(state => state.clearMessages)
  const fontSize = useStore(state => state.fontSize)

  const displayMode = tab?.displayMode || 'ascii'
  const allMessages = tab?.messages || []
  const filterText = tab?.filterText || ''
  const filterIsRegex = tab?.filterIsRegex ?? false
  const framingEnabled = tab?.framingEnabled ?? false
  const framingStart = tab?.framingStart ?? '02'
  const framingEnd = tab?.framingEnd ?? '03'

  const virtuosoRef = useRef<VirtuosoHandle>(null)
  const [autoScroll, setAutoScroll] = useState(true)
  const [showStats, setShowStats] = useState(false)
  const [showPlotter, setShowPlotter] = useState(false)
  const [showFramingConfig, setShowFramingConfig] = useState(false)
  const [showDelta, setShowDelta] = useState(false)

  // ── Filtering ──────────────────────────────────────────────────────────
  const filteredMessages = useMemo(() => {
    if (!filterText) return allMessages
    try {
      if (filterIsRegex) {
        const re = new RegExp(filterText, 'i')
        return allMessages.filter(m => re.test(formatAscii(m.payload)) || re.test(formatHex(m.payload)))
      }
      const lower = filterText.toLowerCase()
      return allMessages.filter(m =>
        formatAscii(m.payload).toLowerCase().includes(lower) ||
        formatHex(m.payload).toLowerCase().includes(lower)
      )
    } catch { return allMessages }
  }, [allMessages, filterText, filterIsRegex])

  // ── Delta timestamps ───────────────────────────────────────────────────
  const deltas = useMemo(() => {
    return filteredMessages.map((msg, i) => {
      if (i === 0) return null
      const prev = filteredMessages[i - 1]
      return new Date(msg.timestamp).getTime() - new Date(prev.timestamp).getTime()
    })
  }, [filteredMessages])

  const scrollToBottom = useCallback(() => {
    virtuosoRef.current?.scrollToIndex({ index: Number.MAX_SAFE_INTEGER, behavior: 'auto' })
  }, [])

  // Always keep ref in sync with latest render — reads in effects are never stale
  const autoScrollRef = useRef(autoScroll)
  autoScrollRef.current = autoScroll

  // Use last message ID so the effect fires even when length is capped at 5000
  const lastMsgId = filteredMessages[filteredMessages.length - 1]?.id
  useEffect(() => {
    // Read from ref, not closure — prevents stale-closure scrolls from queued renders
    if (autoScrollRef.current) scrollToBottom()
  }, [lastMsgId, scrollToBottom])

  if (!tab) return null

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-[#0d1117] relative overflow-hidden">
      {/* Toolbar */}
      <div className="flex items-center px-3 py-1.5 bg-gray-900 border-b border-gray-800 gap-2 shrink-0 flex-wrap">
        {/* Filter input */}
        <div className="flex items-center flex-1 min-w-[160px] bg-gray-800 border border-gray-700 rounded overflow-hidden">
          <input
            type="text"
            value={filterText}
            onChange={e => updateTab(tab.id, { filterText: e.target.value })}
            placeholder="Filter messages…"
            className="flex-1 bg-transparent text-gray-300 text-xs px-3 py-1.5 focus:outline-none placeholder-gray-600 font-mono"
          />
          {filterText && (
            <button onClick={() => updateTab(tab.id, { filterText: '' })} className="px-2 text-gray-500 hover:text-gray-300">
              <X size={12} />
            </button>
          )}
          <button
            onClick={() => updateTab(tab.id, { filterIsRegex: !filterIsRegex })}
            title="Toggle regex"
            className={`px-2 py-1.5 border-l border-gray-700 text-xs transition-colors ${filterIsRegex ? 'text-purple-400 bg-purple-500/10' : 'text-gray-500 hover:text-gray-300'}`}
          >
            <Regex size={13} />
          </button>
        </div>

        {/* Framing config toggle */}
        <button
          onClick={() => setShowFramingConfig(!showFramingConfig)}
          title="Packet framing"
          className={`px-2 py-1.5 text-xs rounded border transition-colors ${framingEnabled ? 'border-amber-600/50 text-amber-400 bg-amber-500/10' : 'border-gray-700 text-gray-500 hover:text-gray-300 bg-gray-800'}`}
        >
          {'{ }'}
        </button>

        {/* Delta time toggle */}
        <button
          onClick={() => setShowDelta(!showDelta)}
          title={showDelta ? 'Hide delta timestamps' : 'Show delta timestamps'}
          className={`px-2 py-1.5 text-xs font-mono rounded border transition-colors ${showDelta ? 'border-yellow-600/50 text-yellow-400 bg-yellow-500/10' : 'border-gray-700 text-gray-500 hover:text-gray-300 bg-gray-800'}`}
        >
          Δt
        </button>

        {/* Stats */}
        <button
          onClick={() => setShowStats(!showStats)}
          title="Statistics"
          className={`p-1.5 rounded border transition-colors ${showStats ? 'border-blue-600/50 text-blue-400 bg-blue-500/10' : 'border-gray-700 text-gray-500 hover:text-gray-300 bg-gray-800'}`}
        >
          <BarChart2 size={14} />
        </button>

        {/* Plotter */}
        <button
          onClick={() => setShowPlotter(!showPlotter)}
          title="Plotter"
          className={`p-1.5 rounded border transition-colors ${showPlotter ? 'border-emerald-600/50 text-emerald-400 bg-emerald-500/10' : 'border-gray-700 text-gray-500 hover:text-gray-300 bg-gray-800'}`}
        >
          <TrendingUp size={14} />
        </button>

        {/* Auto-scroll */}
        <button
          onClick={() => setAutoScroll(prev => !prev)}
          title={autoScroll ? 'Auto-scroll ON' : 'Auto-scroll OFF'}
          className={`flex items-center space-x-1 px-2 py-1.5 text-xs rounded border transition-colors ${autoScroll ? 'bg-emerald-500/15 border-emerald-600/40 text-emerald-400' : 'bg-gray-800 border-gray-700 text-gray-400 hover:text-gray-200'}`}
        >
          <ArrowDownToLine size={12} /><span>{autoScroll ? 'Auto' : 'Manual'}</span>
        </button>

        {/* Clear */}
        <button onClick={() => clearMessages(tab.id)} className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded border border-gray-700 transition-colors">
          Clear
        </button>

        {/* Filter count */}
        {filterText && (
          <span className="text-xs text-gray-500">{filteredMessages.length}/{allMessages.length}</span>
        )}
      </div>

      {/* Framing config row */}
      {showFramingConfig && (
        <div className="bg-gray-900 border-b border-gray-800 px-4 py-2 flex items-center gap-3 text-sm shrink-0">
          <span className="text-xs text-gray-400 font-semibold">Packet Framing:</span>
          <label className="text-xs text-gray-500">Start</label>
          <input
            className="w-16 bg-gray-800 border border-gray-700 text-gray-200 text-xs font-mono px-2 py-1 rounded focus:outline-none focus:border-amber-500"
            value={framingStart}
            onChange={e => updateTab(tab.id, { framingStart: e.target.value.replace(/[^0-9a-fA-F]/g, '').slice(0, 2) })}
            placeholder="02"
          />
          <label className="text-xs text-gray-500">End</label>
          <input
            className="w-16 bg-gray-800 border border-gray-700 text-gray-200 text-xs font-mono px-2 py-1 rounded focus:outline-none focus:border-amber-500"
            value={framingEnd}
            onChange={e => updateTab(tab.id, { framingEnd: e.target.value.replace(/[^0-9a-fA-F]/g, '').slice(0, 2) })}
            placeholder="03"
          />
          <label className="flex items-center gap-2 text-xs text-gray-400 cursor-pointer">
            <input
              type="checkbox"
              checked={framingEnabled}
              onChange={e => updateTab(tab.id, { framingEnabled: e.target.checked })}
              className="accent-amber-400"
            />
            Enable highlighting
          </label>
        </div>
      )}

      {/* Main area: console + optional side stats */}
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Message list */}
        <div className="flex-1 min-h-0 min-w-0">
          {filteredMessages.length === 0 ? (
            <div className="flex items-center justify-center h-full text-gray-600 font-mono text-sm select-none">
              {allMessages.length === 0 ? 'No data yet…' : 'No messages match filter'}
            </div>
          ) : (
            <Virtuoso
              ref={virtuosoRef}
              className="w-full h-full"
              data={filteredMessages}
              followOutput={false}
              itemContent={(index, msg) => (
                <MemoizedRow
                  key={msg.id}
                  msg={msg}
                  displayMode={displayMode}
                  deltaMs={showDelta ? deltas[index] : null}
                  fontSize={fontSize}
                  framingEnabled={framingEnabled}
                  framingStart={framingStart}
                  framingEnd={framingEnd}
                />
              )}
            />
          )}
        </div>

        {/* Stats side panel */}
        {showStats && (
          <StatsPanel tabId={tab.id} messages={allMessages} onClose={() => setShowStats(false)} />
        )}
      </div>

      {/* Plotter panel (bottom) */}
      {showPlotter && (
        <PlotterView messages={allMessages} onClose={() => setShowPlotter(false)} />
      )}
    </div>
  )
}
