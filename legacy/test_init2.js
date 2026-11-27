console.log("[sync] process.type:", process.type);
setImmediate(() => {
  console.log("[setImmediate] process.type:", process.type);
  const e = require("electron");
  console.log("[setImmediate] typeof electron:", typeof e, typeof e === 'object' ? Object.keys(e).slice(0,3).join(', ') : String(e).slice(0,60));
  setTimeout(() => process.exit(0), 500);
});
