import { useState, useRef, useEffect } from 'react'
import { Plus, X, TerminalSquare, ChevronDown, Activity, Monitor } from 'lucide-react'
import { useStore } from './store'
import { TabSession } from './components/TabSession'

export default function App() {
  const tabs = useStore(state => state.tabs)
  const activeTabId = useStore(state => state.activeTabId)
  const addTab = useStore(state => state.addTab)
  const removeTab = useStore(state => state.removeTab)
  const setActiveTab = useStore(state => state.setActiveTab)
  const fontSize = useStore(state => state.fontSize)
  const setFontSize = useStore(state => state.setFontSize)

  const [isAddMenuOpen, setIsAddMenuOpen] = useState(false)
  const addMenuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (addMenuRef.current && !addMenuRef.current.contains(event.target as Node)) {
        setIsAddMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  // Ctrl+Scroll → font size
  useEffect(() => {
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return
      e.preventDefault()
      setFontSize(fontSize + (e.deltaY < 0 ? 1 : -1))
    }
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey) return
      if (e.key === '=' || e.key === '+') { e.preventDefault(); setFontSize(fontSize + 1) }
      if (e.key === '-') { e.preventDefault(); setFontSize(fontSize - 1) }
      if (e.key === '0') { e.preventDefault(); setFontSize(13) }
    }
    window.addEventListener('wheel', onWheel, { passive: false })
    window.addEventListener('keydown', onKey)
    return () => { window.removeEventListener('wheel', onWheel); window.removeEventListener('keydown', onKey) }
  }, [fontSize, setFontSize])

  return (
    <div
      className="flex flex-col h-screen w-screen bg-gray-950 text-white overflow-hidden font-sans"
      style={{ '--console-font-size': `${fontSize}px` } as React.CSSProperties}
    >
      {/* Title Bar / Tabs Header */}
      <div className="flex items-end bg-gray-900 border-b border-gray-800 pt-2 px-2 shrink-0 select-none AppRegionDrag">
        <div className="flex items-center px-4 mb-2 mr-4 text-emerald-400 space-x-2">
          <TerminalSquare size={20} />
          <span className="font-bold tracking-wider text-sm">SerialX</span>
        </div>

        <div className="flex-1 flex overflow-x-auto AppRegionNoDrag hide-scrollbar">
          {tabs.map((tab, idx) => {
            const isActive = tab.id === activeTabId
            return (
              <div 
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`group flex items-center min-w-[140px] max-w-[200px] h-9 px-3 border-t border-x rounded-t-lg transition-colors cursor-pointer mr-1 relative ${
                  isActive 
                    ? 'bg-gray-800 border-gray-700 z-10' 
                    : 'bg-gray-900 border-transparent text-gray-500 hover:bg-gray-800/50 hover:text-gray-300'
                }`}
              >
                {isActive && <div className="absolute top-0 left-0 right-0 h-[2px] bg-emerald-500 rounded-t-lg"></div>}
                
                <div className={`w-2 h-2 rounded-full mr-2 shrink-0 ${tab.isConnected ? 'bg-emerald-500' : 'bg-gray-600'}`}></div>
                
                <span className="truncate flex-1 text-sm font-medium mr-2 flex items-center space-x-1">
                  {tab.tabType === 'modbus' ? <Activity size={14} className="text-purple-400" /> : <Monitor size={14} className="text-blue-400" />}
                  <span>{tab.config.path || `Session ${idx + 1}`}</span>
                </span>
                
                <button 
                  onClick={(e) => { e.stopPropagation(); removeTab(tab.id) }}
                  className={`ml-2 p-1 rounded hover:bg-gray-700 transition-colors ${isActive ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}
                >
                  <X size={14} />
                </button>
              </div>
            )
          })}
        </div>
        
        <div className="relative mb-1 ml-2 flex items-center AppRegionNoDrag" ref={addMenuRef}>
          <button 
            onClick={() => setIsAddMenuOpen(!isAddMenuOpen)}
            className="flex items-center space-x-1 p-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 hover:text-white rounded-md transition-colors"
          >
            <Plus size={16} />
            <ChevronDown size={14} />
          </button>
          
          {isAddMenuOpen && (
            <div className="absolute top-full mt-1 right-0 w-52 bg-gray-800 border border-gray-700 rounded-lg shadow-2xl py-1 z-50 overflow-hidden">
              <button
                className="w-full text-left px-4 py-2 text-sm text-gray-200 hover:bg-gray-700 flex items-center"
                onClick={() => { addTab('generic'); setIsAddMenuOpen(false) }}
              >
                <Monitor size={16} className="mr-2 text-blue-400" />
                Generic Serial
              </button>
              <button
                className="w-full text-left px-4 py-2 text-sm text-gray-200 hover:bg-gray-700 flex items-center"
                onClick={() => { addTab('modbus'); setIsAddMenuOpen(false) }}
              >
                <Activity size={16} className="mr-2 text-purple-400" />
                Modbus RTU
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Tab Content Body */}
      <div className="flex-1 overflow-hidden relative">
        {tabs.map(tab => (
          <div 
            key={tab.id} 
            className={`absolute inset-0 ${tab.id === activeTabId ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'}`}
            aria-hidden={tab.id !== activeTabId}
          >
            {/* Unmount optimization: we might want to keep instances mounted so we don't lose internal scroll state of virtuoso */}
            <TabSession tabId={tab.id} />
          </div>
        ))}
      </div>
    </div>
  )
}
