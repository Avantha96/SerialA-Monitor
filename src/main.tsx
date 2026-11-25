import React from 'react'
import ReactDOM from 'react-dom/client'
import './api'          // installs window.electronAPI on top of Tauri IPC
import App from './App'
import './index.css'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
