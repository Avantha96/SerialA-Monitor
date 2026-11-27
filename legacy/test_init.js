console.log("process.type:", process.type);
console.log("process.versions.electron:", process.versions.electron);
const e = require("electron");
console.log("typeof electron:", typeof e, typeof e === 'object' ? Object.keys(e).slice(0,5).join(', ') : String(e).slice(0,60));
setTimeout(() => process.exit(0), 1000);
