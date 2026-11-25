console.log("process.versions.electron:", process.versions.electron);
console.log("process.type:", process.type);

// Try native binding approach
try {
  const binding = process.electronBinding('app');
  console.log("electronBinding works:", !!binding);
} catch(e) {
  console.log("electronBinding error:", e.message);
}

// Try _linkedBinding
try {
  const b = process._linkedBinding('electron_browser_app');
  console.log("_linkedBinding works:", !!b);
} catch(e) {
  console.log("_linkedBinding error:", e.message);
}

setTimeout(() => process.exit(0), 1000);
