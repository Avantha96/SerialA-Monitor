import { useEffect, useMemo, useState } from 'react'
import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'
import { X } from 'lucide-react'
import { SerialMessage } from '../store'

interface StatsPanelProps {
  tabId: string
  messages: SerialMessage[]
  onClose: () => void
}

const POLL_MS = 500

/**
 * Counters come from Rust. The previous build re-scanned the whole 5000-message
 * ring buffer (and every byte of it) on each render just to draw this panel;
 * the backend now accumulates totals, a 1 s rate window and the byte histogram
 * as data arrives, so opening Statistics no longer costs the render loop.
 * These figures also cover the entire session, not just the visible buffer.
 */
export function StatsPanel({ tabId, messages, onClose }: StatsPanelProps) {
  const [stats, setStats] = useState<SessionStats | null>(null)

  useEffect(() => {
    let alive = true
    const poll = async () => {
      const s = await window.electronAPI.getStats(tabId)
      if (alive) setStats(s)
    }
    poll()
    const timer = setInterval(poll, POLL_MS)
    return () => { alive = false; clearInterval(timer) }
  }, [tabId])

  const freqData = useMemo(
    () =>
      (stats?.topBytes ?? []).map(({ byte, count }) => ({
        byte: `0x${byte.toString(16).padStart(2, '0').toUpperCase()}`,
        count,
      })),
    [stats]
  )

  const fmt = (n: number) => {
    if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(2)} MB`
    if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`
    return `${n} B`
  }

  const counters = [
    { label: 'RX Total', value: fmt(stats?.totalRx ?? 0), color: 'text-emerald-400' },
    { label: 'TX Total', value: fmt(stats?.totalTx ?? 0), color: 'text-blue-400' },
    { label: 'RX Rate', value: `${stats?.rxRate ?? 0} B/s`, color: 'text-emerald-300' },
    { label: 'TX Rate', value: `${stats?.txRate ?? 0} B/s`, color: 'text-blue-300' },
    { label: 'Buffered', value: String(messages.length), color: 'text-gray-300' },
    { label: 'RX Chunks', value: String(stats?.rxChunks ?? 0), color: 'text-gray-400' },
  ]

  return (
    <div className="bg-gray-900 border-l border-gray-700 w-72 shrink-0 flex flex-col text-sm">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-gray-700">
        <span className="font-semibold text-gray-200">Statistics</span>
        <button onClick={onClose} className="text-gray-500 hover:text-gray-200 transition-colors"><X size={16} /></button>
      </div>

      {/* Counters */}
      <div className="grid grid-cols-2 gap-2 p-4">
        {counters.map(({ label, value, color }) => (
          <div key={label} className="bg-gray-800 rounded-lg p-2">
            <div className="text-xs text-gray-500 mb-1">{label}</div>
            <div className={`font-mono font-bold ${color}`}>{value}</div>
          </div>
        ))}
      </div>

      {/* Byte Frequency */}
      <div className="px-4 pb-4 flex-1">
        <div className="text-xs text-gray-500 uppercase tracking-wider mb-2">Top RX Byte Frequency</div>
        {freqData.length === 0 ? (
          <div className="text-gray-600 text-xs text-center py-4">
            {stats ? 'No RX data yet' : 'Connect to collect statistics'}
          </div>
        ) : (
          <ResponsiveContainer width="100%" height={160}>
            <BarChart data={freqData} margin={{ top: 4, right: 4, left: -20, bottom: 4 }}>
              <XAxis dataKey="byte" tick={{ fontSize: 9, fill: '#6b7280' }} />
              <YAxis tick={{ fontSize: 9, fill: '#6b7280' }} />
              <Tooltip
                contentStyle={{ background: '#1f2937', border: '1px solid #374151', fontSize: 11 }}
                labelStyle={{ color: '#d1d5db' }}
                itemStyle={{ color: '#34d399' }}
              />
              <Bar dataKey="count" fill="#10b981" radius={[3, 3, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        )}
      </div>
    </div>
  )
}
