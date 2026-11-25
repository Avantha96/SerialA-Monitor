import { useMemo, useState } from 'react'
import {
  LineChart, Line, XAxis, YAxis, Tooltip, Legend, ResponsiveContainer,
  AreaChart, Area, CartesianGrid
} from 'recharts'
import { X, TrendingUp, Activity } from 'lucide-react'
import { SerialMessage } from '../store'

const PALETTE = ['#10b981', '#3b82f6', '#f59e0b', '#ef4444', '#a855f7', '#ec4899']
const MAX_POINTS = 200

interface PlotterViewProps {
  messages: SerialMessage[]
  onClose: () => void
}

type PlotTab = 'csv' | 'byterate'

interface CsvPoint {
  t: number
  [series: string]: number
}

interface RatePoint {
  t: number
  bps: number
}

function parseCsvFromMessages(messages: SerialMessage[]): { points: CsvPoint[], series: string[] } {
  const rxMessages = messages.filter(m => !m.isSent)
  const points: CsvPoint[] = []
  let series: string[] = []
  let autoHeaders: string[] | null = null

  for (const msg of rxMessages) {
    const line = new TextDecoder().decode(msg.payload).trim()
    if (!line) continue

    // Detect optional header row like "x,y,z"
    const parts = line.split(',')
    const nums = parts.map(p => parseFloat(p.trim()))
    const allNum = nums.every(n => !isNaN(n))

    if (!allNum) {
      // Treat as header
      autoHeaders = parts.map(p => p.trim())
      series = autoHeaders
      continue
    }

    if (series.length === 0) {
      series = nums.map((_, i) => `ch${i + 1}`)
    }

    const point: CsvPoint = { t: new Date(msg.timestamp).getTime() }
    series.forEach((key, i) => { if (nums[i] !== undefined) point[key] = nums[i] })
    points.push(point)
  }

  return { points: points.slice(-MAX_POINTS), series: series.slice(0, 6) }
}

function computeByteRate(messages: SerialMessage[]): RatePoint[] {
  if (messages.length === 0) return []
  const buckets = new Map<number, number>()
  for (const msg of messages) {
    const sec = Math.floor(new Date(msg.timestamp).getTime() / 1000)
    buckets.set(sec, (buckets.get(sec) ?? 0) + msg.payload.length)
  }
  return Array.from(buckets.entries())
    .sort(([a], [b]) => a - b)
    .slice(-MAX_POINTS)
    .map(([sec, bps]) => ({ t: sec, bps }))
}

export function PlotterView({ messages, onClose }: PlotterViewProps) {
  const [activeTab, setActiveTab] = useState<PlotTab>('csv')
  const { points, series } = useMemo(() => parseCsvFromMessages(messages), [messages])
  const ratePoints = useMemo(() => computeByteRate(messages), [messages])

  const fmtTime = (t: any): string => {
    if (!t) return ''
    if (activeTab === 'byterate') {
      return new Date(Number(t) * 1000).toLocaleTimeString([], { hour12: false })
    }
    return new Date(Number(t)).toLocaleTimeString([], { hour12: false })
  }

  return (
    <div className="bg-gray-900 border-t border-gray-700 h-56 shrink-0 flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center px-3 py-1.5 border-b border-gray-800 gap-2 shrink-0">
        <div className="flex rounded overflow-hidden border border-gray-700">
          <button
            onClick={() => setActiveTab('csv')}
            className={`flex items-center space-x-1 px-3 py-1 text-xs font-medium transition-colors ${activeTab === 'csv' ? 'bg-emerald-500/20 text-emerald-400' : 'text-gray-500 hover:text-gray-300'}`}
          >
            <TrendingUp size={12} /><span>Plotter</span>
          </button>
          <button
            onClick={() => setActiveTab('byterate')}
            className={`flex items-center space-x-1 px-3 py-1 text-xs font-medium border-l border-gray-700 transition-colors ${activeTab === 'byterate' ? 'bg-blue-500/20 text-blue-400' : 'text-gray-500 hover:text-gray-300'}`}
          >
            <Activity size={12} /><span>Byte Rate</span>
          </button>
        </div>

        {activeTab === 'csv' && (
          <span className="text-xs text-gray-500">
            {series.length > 0
              ? `${series.length} series: ${series.join(', ')} — ${points.length} pts`
              : 'No CSV data yet — send comma-separated floats'}
          </span>
        )}

        <button onClick={onClose} className="ml-auto text-gray-500 hover:text-gray-200 transition-colors"><X size={16} /></button>
      </div>

      {/* Chart */}
      <div className="flex-1 min-h-0 p-2">
        {activeTab === 'csv' ? (
          points.length === 0 ? (
            <div className="flex items-center justify-center h-full text-gray-600 text-xs">
              Incoming lines like <code className="mx-1 text-gray-400">1.23,4.56,7.89</code> will be plotted here
            </div>
          ) : (
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={points}>
                <CartesianGrid strokeDasharray="3 3" stroke="#1f2937" />
                <XAxis dataKey="t" tickFormatter={fmtTime} tick={{ fontSize: 9, fill: '#6b7280' }} />
                <YAxis tick={{ fontSize: 9, fill: '#6b7280' }} width={40} />
                <Tooltip
                  contentStyle={{ background: '#111827', border: '1px solid #374151', fontSize: 11 }}
                  labelFormatter={fmtTime}
                />
                <Legend iconSize={8} wrapperStyle={{ fontSize: 10 }} />
                {series.map((s, i) => (
                  <Line
                    key={s} type="monotone" dataKey={s}
                    stroke={PALETTE[i % PALETTE.length]}
                    dot={false} strokeWidth={1.5} isAnimationActive={false}
                  />
                ))}
              </LineChart>
            </ResponsiveContainer>
          )
        ) : (
          ratePoints.length === 0 ? (
            <div className="flex items-center justify-center h-full text-gray-600 text-xs">Waiting for data…</div>
          ) : (
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={ratePoints}>
                <CartesianGrid strokeDasharray="3 3" stroke="#1f2937" />
                <XAxis dataKey="t" tickFormatter={fmtTime} tick={{ fontSize: 9, fill: '#6b7280' }} />
                <YAxis tick={{ fontSize: 9, fill: '#6b7280' }} width={40} unit=" B/s" />
                <Tooltip
                  contentStyle={{ background: '#111827', border: '1px solid #374151', fontSize: 11 }}
                  labelFormatter={fmtTime}
                  formatter={(v: any) => [`${v} B/s`, 'Byte Rate']}
                />
                <Area type="monotone" dataKey="bps" stroke="#3b82f6" fill="#1d4ed8" fillOpacity={0.3} dot={false} strokeWidth={1.5} isAnimationActive={false} />
              </AreaChart>
            </ResponsiveContainer>
          )
        )}
      </div>
    </div>
  )
}
