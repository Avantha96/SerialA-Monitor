import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export interface SerialConfig {
  path: string
  baudRate: number
  dataBits: 8 | 7 | 6 | 5
  stopBits: 1 | 2
  parity: 'none' | 'even' | 'mark' | 'odd' | 'space'
}

export interface SerialMessage {
  id: string
  timestamp: string
  payload: Uint8Array
  isSent: boolean
}

export interface TabSession {
  id: string
  name: string
  tabType: 'generic' | 'modbus'
  config: SerialConfig
  isConnected: boolean
  isModbusMode: boolean
  displayMode: 'ascii' | 'hex' | 'both'
  isLogging: boolean
  messages: SerialMessage[]
  autoReconnect: boolean

  // Analysis
  filterText: string
  filterIsRegex: boolean
  framingEnabled: boolean
  framingStart: string  // hex byte e.g. "02"
  framingEnd: string    // hex byte e.g. "03"

  // Interval sending state
  intervalData: string
  intervalDelayMs: number
  intervalActive: boolean
}

interface AppState {
  tabs: TabSession[]
  activeTabId: string | null
  fontSize: number

  addTab: (type?: 'generic' | 'modbus') => void
  removeTab: (id: string) => void
  setActiveTab: (id: string) => void
  updateTab: (id: string, updates: Partial<TabSession>) => void
  addMessage: (id: string, message: Omit<SerialMessage, 'id'>) => void
  addMessages: (id: string, batch: Omit<SerialMessage, 'id'>[]) => void
  clearMessages: (id: string) => void
  setFontSize: (size: number) => void
}

const DEFAULT_CONFIG: SerialConfig = {
  path: '',
  baudRate: 9600,
  dataBits: 8,
  stopBits: 1,
  parity: 'none'
}

const createInitialTab = (index: number, type: 'generic' | 'modbus' = 'generic'): TabSession => ({
  id: `tab-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`,
  name: `Session ${index}`,
  tabType: type,
  config: { ...DEFAULT_CONFIG },
  isConnected: false,
  isModbusMode: false,
  displayMode: 'ascii',
  isLogging: false,
  messages: [],
  autoReconnect: false,
  filterText: '',
  filterIsRegex: false,
  framingEnabled: false,
  framingStart: '02',
  framingEnd: '03',
  intervalData: '',
  intervalDelayMs: 1000,
  intervalActive: false
})

// Helper to rehydrate Uint8Array (not serializable as-is)
function rehydrateTabs(tabs: TabSession[]): TabSession[] {
  return tabs.map(t => ({
    ...t,
    isConnected: false,       // never persist connected state
    intervalActive: false,
    messages: [],             // never persist messages (too large)
    filterText: t.filterText ?? '',
    filterIsRegex: t.filterIsRegex ?? false,
    framingEnabled: t.framingEnabled ?? false,
    framingStart: t.framingStart ?? '02',
    framingEnd: t.framingEnd ?? '03',
    autoReconnect: t.autoReconnect ?? false,
  }))
}

export const useStore = create<AppState>()(
  persist(
    (set) => {
      const initialTab = createInitialTab(1, 'generic')

      return {
        tabs: [initialTab],
        activeTabId: initialTab.id,
        fontSize: 13,

        addTab: (type = 'generic') => set((state) => {
          const newTab = createInitialTab(state.tabs.length + 1, type)
          return { tabs: [...state.tabs, newTab], activeTabId: newTab.id }
        }),

        removeTab: (id) => set((state) => {
          const filteredTabs = state.tabs.filter(t => t.id !== id)
          if (filteredTabs.length === 0) {
            const fallback = createInitialTab(1, 'generic')
            return { tabs: [fallback], activeTabId: fallback.id }
          }
          const newActive = state.activeTabId === id ? filteredTabs[0].id : state.activeTabId
          return { tabs: filteredTabs, activeTabId: newActive }
        }),

        setActiveTab: (id) => set({ activeTabId: id }),

        updateTab: (id, updates) => set((state) => ({
          tabs: state.tabs.map(t => t.id === id ? { ...t, ...updates } : t)
        })),

        addMessage: (id, message) => set((state) => ({
          tabs: state.tabs.map(t => {
            if (t.id === id) {
              const newMsg = { ...message, id: Math.random().toString(36).substring(2, 9) }
              // Cap message buffer at 5000 to prevent memory issues
              const messages = t.messages.length >= 5000
                ? [...t.messages.slice(-4999), newMsg]
                : [...t.messages, newMsg]
              return { ...t, messages }
            }
            return t
          })
        })),

        addMessages: (id, batch) => set((state) => ({
          tabs: state.tabs.map(t => {
            if (t.id !== id) return t
            const newMsgs = batch.map(m => ({ ...m, id: Math.random().toString(36).substring(2, 9) }))
            const combined = [...t.messages, ...newMsgs]
            const messages = combined.length > 5000 ? combined.slice(-5000) : combined
            return { ...t, messages }
          })
        })),

        clearMessages: (id) => set((state) => ({
          tabs: state.tabs.map(t => t.id === id ? { ...t, messages: [] } : t)
        })),

        setFontSize: (size) => set({ fontSize: Math.max(10, Math.min(22, size)) }),
      }
    },
    {
      name: 'serialx-sessions',
      // Only persist config-level data, not live runtime state
      partialize: (state) => ({
        fontSize: state.fontSize,
        activeTabId: state.activeTabId,
        tabs: state.tabs.map(t => ({
          id: t.id,
          name: t.name,
          tabType: t.tabType,
          config: t.config,
          displayMode: t.displayMode,
          autoReconnect: t.autoReconnect,
          framingEnabled: t.framingEnabled,
          framingStart: t.framingStart,
          framingEnd: t.framingEnd,
          filterText: '',
          filterIsRegex: false,
          // Required fields with defaults for rehydration
          isConnected: false,
          isModbusMode: false,
          isLogging: false,
          messages: [],
          intervalData: '',
          intervalDelayMs: 1000,
          intervalActive: false,
        }))
      }),
      merge: (persisted: any, current) => ({
        ...current,
        ...(persisted || {}),
        tabs: persisted?.tabs ? rehydrateTabs(persisted.tabs) : current.tabs,
        fontSize: persisted?.fontSize ?? current.fontSize,
      })
    }
  )
)
