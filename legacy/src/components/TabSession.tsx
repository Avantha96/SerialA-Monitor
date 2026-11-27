import { useEffect, useRef } from 'react'
import { ConfigPanel } from './ConfigPanel'
import { ConsoleView } from './ConsoleView'
import { IntervalSender } from './IntervalSender'
import { ModbusView } from './ModbusView'
import { useStore } from '../store'

export function TabSession({ tabId }: { tabId: string }) {
  const addMessage = useStore(state => state.addMessage)
  const addMessages = useStore(state => state.addMessages)
  const updateTab = useStore(state => state.updateTab)
  const tab = useStore(state => state.tabs.find(t => t.id === tabId))
  const isModbus = tab?.tabType === 'modbus'
  const autoReconnect = tab?.autoReconnect ?? false

  // Batch buffer for high-speed incoming data
  const pendingRef = useRef<{ timestamp: string; payload: Uint8Array; isSent: false }[]>([])
  const rafRef = useRef<number | null>(null)

  // Sync autoReconnect setting to backend whenever it changes
  useEffect(() => {
    if (window.electronAPI?.setAutoReconnect) {
      window.electronAPI.setAutoReconnect(tabId, autoReconnect)
    }
  }, [tabId, autoReconnect])

  useEffect(() => {
    if (!window.electronAPI) return

    const flushPending = () => {
      if (pendingRef.current.length > 0) {
        addMessages(tabId, pendingRef.current)
        pendingRef.current = []
      }
      rafRef.current = null
    }

    // Register IPC listeners
    const unsubscribeData = window.electronAPI.onData((id, data) => {
      if (id !== tabId) return
      pendingRef.current.push({ timestamp: new Date().toISOString(), payload: data, isSent: false })
      if (rafRef.current === null) {
        rafRef.current = requestAnimationFrame(flushPending)
      }
    })

    const unsubscribeError = window.electronAPI.onError((id, err) => {
      if (id === tabId) addMessage(id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode(`ERROR: ${err}`), isSent: false })
    })

    const unsubscribeClosed = window.electronAPI.onClosed((id) => {
      if (id === tabId) {
        updateTab(id, { isConnected: false })
        addMessage(id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode('Port closed.'), isSent: false })
      }
    })

    const unsubscribeReconnecting = window.electronAPI.onReconnecting((id) => {
      if (id === tabId) {
        addMessage(id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode('⟳ Connection lost — reconnecting…'), isSent: false })
      }
    })

    const unsubscribeReconnected = window.electronAPI.onReconnected((id) => {
      if (id === tabId) {
        updateTab(id, { isConnected: true })
        addMessage(id, { timestamp: new Date().toISOString(), payload: new TextEncoder().encode('✓ Reconnected successfully.'), isSent: false })
      }
    })

    return () => {
      unsubscribeData()
      unsubscribeError()
      unsubscribeClosed()
      unsubscribeReconnecting()
      unsubscribeReconnected()
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current)
      pendingRef.current = []
    }
  }, [tabId, addMessage, addMessages, updateTab])

  return (
    <div className="flex flex-col h-full w-full bg-gray-900">
      <ConfigPanel tabId={tabId} />
      {isModbus ? (
        <ModbusView tabId={tabId} />
      ) : (
        <>
          <ConsoleView tabId={tabId} />
          <IntervalSender tabId={tabId} />
        </>
      )}
    </div>
  )
}
