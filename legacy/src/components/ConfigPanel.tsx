import { useEffect, useState } from 'react'
import { useStore, SerialConfig } from '../store'
import { Play, Square, Save, RefreshCw, Type, Hash, SplitSquareHorizontal, Wifi } from 'lucide-react'

export function ConfigPanel({ tabId }: { tabId: string }) {
  const tab = useStore(state => state.tabs.find(t => t.id === tabId))
  const updateTab = useStore(state => state.updateTab)
  const addMessage = useStore(state => state.addMessage)
  const [ports, setPorts] = useState<{path: string, manufacturer: string}[]>([])

  const loadPorts = () => {
    if (window.electronAPI) {
      window.electronAPI.getPorts().then(setPorts)
    }
  }

  useEffect(() => {
    loadPorts()
  }, [])

  if (!tab) return null
  const { config, isConnected, displayMode, messages, tabType, autoReconnect } = tab

  const handleConnectToggle = async () => {
    if (!window.electronAPI) return

    if (isConnected) {
      window.electronAPI.disconnect(tab.id)
      updateTab(tab.id, { isConnected: false })
      addMessage(tab.id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode('Disconnected.'), isSent: false })
    } else {
      if (!config.path) {
        addMessage(tab.id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode('Error: Please select a valid COM port.'), isSent: false })
        return
      }
      
      // `mode` tells the Rust backend whether to feed the console or the RTU parser.
      const res = await window.electronAPI.connect(tab.id, { ...config, mode: tabType })
      if (typeof res === 'object' && res.error) {
        addMessage(tab.id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode(`Error: ${res.error}`), isSent: false })
      } else {
        updateTab(tab.id, { isConnected: true })
        addMessage(tab.id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode(`Connected to ${config.path} @ ${config.baudRate}`), isSent: false })
      }
    }
  }

  const updateConfig = (key: keyof SerialConfig, val: any) => {
    updateTab(tab.id, { config: { ...config, [key]: val } })
  }

  const handleSaveLog = async () => {
    if (!window.electronAPI || messages.length === 0) return
    
    // Generate text content
    const textContent = messages.map(msg => {
      const rawDate = new Date(msg.timestamp)
      const time = rawDate.toLocaleTimeString([], { hour12: false }) + '.' + rawDate.getMilliseconds().toString().padStart(3, '0')
      const prefix = msg.isSent ? 'TX:' : 'RX:'
      let dataStr = ''
      
      if (displayMode === 'hex') {
         dataStr = Array.from(msg.payload).map(b => b.toString(16).padStart(2, '0').toUpperCase()).join(' ')
      } else if (displayMode === 'both') {
         const ascii = new TextDecoder().decode(msg.payload)
         const hex = Array.from(msg.payload).map(b => b.toString(16).padStart(2, '0').toUpperCase()).join(' ')
         dataStr = `[ASC] ${ascii} | [HEX] ${hex}`
      } else {
         dataStr = new TextDecoder().decode(msg.payload)
      }
      return `[${time}] ${prefix} ${dataStr}`
    }).join('\n')

    const now = new Date()
    const ts = `${now.getFullYear()}${String(now.getMonth()+1).padStart(2,'0')}${String(now.getDate()).padStart(2,'0')}${String(now.getHours()).padStart(2,'0')}${String(now.getMinutes()).padStart(2,'0')}`
    const portName = config.path ? config.path.replace(/[^a-zA-Z0-9]/g, '') : 'log'
    const defaultName = `${portName}_${ts}.txt`
    
    const res = await window.electronAPI.saveSession(defaultName, textContent)
    if (res.success) {
      addMessage(tab.id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode(`Session saved to ${res.filePath}`), isSent: false })
    } else if (res.error) {
      addMessage(tab.id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode(`Failed to save: ${res.error}`), isSent: false })
    }
  }

  return (
    <div className="bg-gray-800 border-b border-gray-700 p-4 flex flex-wrap gap-4 items-end shadow-md">
      <div className="flex flex-col space-y-1">
        <label className="text-xs text-gray-400 font-semibold uppercase tracking-wider">Port</label>
        <div className="flex items-center space-x-1">
          <select 
            disabled={isConnected}
            className="bg-gray-700 text-white text-sm rounded-md px-3 py-2 border border-gray-600 focus:outline-none focus:border-blue-500 transition-colors"
            value={config.path}
            onChange={(e) => updateConfig('path', e.target.value)}
          >
            <option value="">Select Port</option>
            {ports.map(p => <option key={p.path} value={p.path}>{p.path} {p.manufacturer ? `(${p.manufacturer})` : ''}</option>)}
          </select>
          <button 
            type="button" 
            onClick={loadPorts} 
            title="Refresh Ports"
            className="p-2 bg-gray-700 border border-gray-600 rounded-md text-gray-300 hover:text-white hover:bg-gray-600 transition-colors"
          >
            <RefreshCw size={16} />
          </button>
        </div>
      </div>
      
      <div className="flex flex-col space-y-1">
        <label className="text-xs text-gray-400 font-semibold uppercase tracking-wider">Baud</label>
        <select 
          disabled={isConnected}
          className="bg-gray-700 text-white text-sm rounded-md px-3 py-2 border border-gray-600 focus:outline-none focus:border-blue-500"
          value={config.baudRate}
          onChange={(e) => updateConfig('baudRate', parseInt(e.target.value))}
        >
          {[9600, 14400, 19200, 38400, 57600, 115200, 128000, 256000, 512000, 1000000].map(b => (
            <option key={b} value={b}>{b}</option>
          ))}
        </select>
      </div>

      <div className="flex flex-col space-y-1">
        <label className="text-xs text-gray-400 font-semibold uppercase tracking-wider">Parity</label>
        <select disabled={isConnected} className="bg-gray-700 text-white text-sm rounded-md px-3 py-2 border border-gray-600" value={config.parity} onChange={(e) => updateConfig('parity', e.target.value)}>
          <option value="none">None</option>
          <option value="even">Even</option>
          <option value="odd">Odd</option>
          <option value="mark">Mark</option>
          <option value="space">Space</option>
        </select>
      </div>

      <div className="flex items-center space-x-2 pl-4 border-l border-gray-700 ml-auto">
        {tabType === 'generic' && (
          <div className="flex bg-gray-900 rounded-md border border-gray-700 overflow-hidden" title="Display Mode">
            <button
              onClick={() => updateTab(tab.id, { displayMode: 'ascii' })}
              className={`px-3 py-2 flex items-center space-x-1 text-xs font-semibold transition-colors ${displayMode === 'ascii' ? 'bg-emerald-500/20 text-emerald-400' : 'text-gray-400 hover:bg-gray-800'}`}
            >
              <Type size={14} /> <span>ASCII</span>
            </button>
            <button
              onClick={() => updateTab(tab.id, { displayMode: 'hex' })}
              className={`px-3 py-2 flex items-center space-x-1 text-xs font-semibold border-l border-gray-700 transition-colors ${displayMode === 'hex' ? 'bg-blue-500/20 text-blue-400' : 'text-gray-400 hover:bg-gray-800'}`}
            >
              <Hash size={14} /> <span>HEX</span>
            </button>
            <button
              onClick={() => updateTab(tab.id, { displayMode: 'both' })}
              className={`px-3 py-2 flex items-center space-x-1 text-xs font-semibold border-l border-gray-700 transition-colors ${displayMode === 'both' ? 'bg-amber-500/20 text-amber-400' : 'text-gray-400 hover:bg-gray-800'}`}
            >
              <SplitSquareHorizontal size={14} /> <span>BOTH</span>
            </button>
          </div>
        )}

        <button 
          onClick={handleConnectToggle}
          className={`flex items-center space-x-2 px-4 py-2 rounded-md font-medium transition-all ${
            isConnected 
              ? 'bg-red-500/10 text-red-400 border border-red-500/30 hover:bg-red-500/20' 
              : 'bg-emerald-500 text-gray-950 hover:bg-emerald-400 shadow-[0_0_15px_rgba(16,185,129,0.4)]'
          }`}
        >
          {isConnected ? <Square size={16} className="mr-1" /> : <Play size={16} className="mr-1" />}
          {isConnected ? 'Disconnect' : 'Connect'}
        </button>

        {tabType === 'generic' && (
          <>
            <button 
              title="Save Session Log"
              disabled={messages.length === 0}
              onClick={handleSaveLog}
              className="p-2 rounded-md bg-gray-700 text-gray-300 hover:text-white hover:bg-gray-600 transition-colors disabled:opacity-50"
            >
              <Save size={18} />
            </button>
            <button
              title={autoReconnect ? 'Auto-reconnect ON' : 'Auto-reconnect OFF'}
              onClick={() => updateTab(tab.id, { autoReconnect: !autoReconnect })}
              className={`p-2 rounded-md transition-colors ${autoReconnect ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-600/40' : 'bg-gray-700 text-gray-400 hover:text-gray-200'}`}
            >
              <Wifi size={18} />
            </button>
          </>
        )}
      </div>
    </div>
  )
}
