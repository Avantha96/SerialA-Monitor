import { app, BrowserWindow } from 'electron';
console.log("process.versions.electron:", process.versions.electron);
console.log("process.type:", process.type);
console.log("app:", typeof app);
app.whenReady().then(() => {
  console.log("App ready!");
  app.quit();
});
