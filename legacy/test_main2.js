// Try different ways to access electron API
console.log("process.versions.electron:", process.versions.electron);
console.log("process.type:", process.type);

// Try module.builtinModules
const Module = require('module');
const builtins = Module.builtinModules || [];
const electronBuiltins = builtins.filter(m => m.includes('electron'));
console.log("electron builtins:", electronBuiltins.join(', ') || 'none');

// Try all possible paths
const paths = ['electron/main', 'electron/common', '@electron/internal/browser/api/exports/electron'];
for (const p of paths) {
  try {
    const m = require(p);
    console.log(p, "->", typeof m, m && typeof m === 'object' ? Object.keys(m).slice(0,5).join(', ') : String(m).slice(0,50));
  } catch(e) {
    console.log(p, "->", e.message.slice(0, 60));
  }
}
setTimeout(() => process.exit(0), 500);
