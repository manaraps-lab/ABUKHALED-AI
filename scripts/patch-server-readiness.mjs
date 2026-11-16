#!/usr/bin/env node
/**
 * ABUKHALED AI startup integration patch for pinned OmniRoute 3.8.53.
 * 1) Uses OmniRoute's lifecycle readiness endpoint for startup gating.
 * 2) Shows a local, branded startup screen immediately while the embedded
 *    server initializes, then navigates the same BrowserWindow to the dashboard.
 * 3) Makes the packaged smoke test wait for the dashboard (not just the splash).
 * The upstream engine, API routes, appId, and user-data paths are preserved.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(process.argv[2] || 'upstream');
const electronDir = path.join(root, 'electron');
const packagePath = path.join(electronDir, 'package.json');
const helperPath = path.join(electronDir, 'lib', 'serverReadiness.js');
const mainPath = path.join(electronDir, 'main.js');
const smokePath = path.join(root, 'scripts', 'dev', 'smoke-electron-packaged.mjs');
const splashSourcePath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'assets', 'startup.html');
const splashTargetPath = path.join(electronDir, 'assets', 'startup.html');
const BRAND = 'ABUKHALED AI';

function fail(message) {
  console.error(`[${BRAND} startup patch] ERROR: ${message}`);
  process.exit(1);
}
function read(filePath) {
  if (!fs.existsSync(filePath)) fail(`Required source file not found: ${filePath}`);
  return fs.readFileSync(filePath, 'utf8').replace(/\r\n?/g, '\n');
}
function replaceExactlyOnce(source, before, after, label) {
  const beforeCount = source.split(before).length - 1;
  const afterCount = source.split(after).length - 1;
  if (beforeCount === 0 && afterCount === 1) return source;
  if (beforeCount !== 1 || afterCount !== 0) {
    fail(`${label}: expected exactly one original match or one already-patched match; found original=${beforeCount}, patched=${afterCount}`);
  }
  return source.replace(before, after);
}

const pkg = JSON.parse(read(packagePath));
if (pkg.version !== '3.8.53') fail(`Expected Electron package version 3.8.53; found ${pkg.version}`);
if (pkg.build?.appId !== 'online.omniroute.desktop') {
  fail(`Unexpected appId; refusing to modify source: ${pkg.build?.appId ?? '(missing)'}`);
}

// Patch the server probe without changing the liveness endpoint itself.
let readiness = read(helperPath);
readiness = replaceExactlyOnce(
  readiness,
  'return `${baseUrl.replace(/\\/+$/, "")}/api/health/ping`;',
  'return `${baseUrl.replace(/\\/+$/, "")}/healthz`;',
  'startup readiness endpoint'
);
if (!readiness.includes('/healthz') || readiness.includes('/api/health/ping')) {
  fail('Startup readiness endpoint postcondition failed.');
}
fs.writeFileSync(helperPath, readiness, 'utf8');

// Ensure the static splash is included in the packaged app's resources.
if (!fs.existsSync(splashSourcePath)) fail(`Startup screen asset not found: ${splashSourcePath}`);
const resources = pkg.build?.extraResources;
if (!Array.isArray(resources)) fail('electron-builder extraResources configuration is missing.');
const assetsResource = resources.find((item) => item?.from === 'assets' && item?.to === 'assets');
if (!assetsResource || !Array.isArray(assetsResource.filter)) {
  fail('Could not locate the existing assets -> assets extraResources rule.');
}
if (!assetsResource.filter.includes('startup.html')) assetsResource.filter.push('startup.html');
fs.mkdirSync(path.dirname(splashTargetPath), { recursive: true });
fs.copyFileSync(splashSourcePath, splashTargetPath);

// Patch the branded Electron main process. These anchors are deliberately strict:
// a changed upstream source must fail closed rather than receive a guessed edit.
let main = read(mainPath);
main = replaceExactlyOnce(
  main,
  'function createWindow({ showWhenReady = true } = {}) {',
  'function createWindow({ showWhenReady = true, startupSplash = false } = {}) {',
  'createWindow startup-splash option'
);
main = replaceExactlyOnce(
  main,
  '    console.log("[ABUKHALED AI] Renderer did-finish-load:", window.webContents.getURL());',
  `    const loadedUrl = window.webContents.getURL();
    if (loadedUrl.startsWith("file://")) {
      console.log("[ABUKHALED AI] Startup splash loaded:", loadedUrl);
    } else {
      console.log("[ABUKHALED AI] Dashboard renderer loaded:", loadedUrl);
    }`,
  'distinguish splash load from dashboard load'
);
const oldInitialNavigation = `  const initialUrl = resolveRendererUrl(lastRendererUrl, getServerUrl());
  window.loadURL(initialUrl).catch((error) => {
    const message = error instanceof Error ? error.message : String(error);
    console.error("[ABUKHALED AI] Initial renderer navigation failed:", message);
    void recoverRenderer(message);
  });`;
const newInitialNavigation = `  if (startupSplash) {
    const splashPath = path.join(RESOURCES_PATH, "assets", "startup.html");
    window.loadFile(splashPath).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      console.error("[ABUKHALED AI] Startup screen navigation failed:", message);
      void recoverRenderer(message);
    });
  } else {
    const initialUrl = resolveRendererUrl(lastRendererUrl, getServerUrl());
    window.loadURL(initialUrl).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      console.error("[ABUKHALED AI] Initial renderer navigation failed:", message);
      void recoverRenderer(message);
    });
  }`;
if (main.includes('if (startupSplash) {') && main.includes('window.loadFile(splashPath)')) {
  // Already patched; retain idempotency while still checking the complete branch.
  if (!main.includes(newInitialNavigation)) {
    fail('Startup splash navigation appears partially modified.');
  }
} else {
  main = replaceExactlyOnce(main, oldInitialNavigation, newInitialNavigation, 'initial BrowserWindow navigation');
}
main = replaceExactlyOnce(
  main,
  '  // Fix #1: Start server and WAIT for readiness before showing window\n  startNextServer();',
  `  // ABUKHALED_PATCH_EARLY_STARTUP_WINDOW_V1
  // Show a lightweight local screen immediately; never leave a blank Electron window
  // while the embedded Next.js server performs first-run initialization.
  if (!isHeadless && !startHidden) {
    createWindow({ showWhenReady: true, startupSplash: true });
  }
  startNextServer();`,
  'create early startup window before server initialization'
);
const oldWindowDecision = `  if (isHeadless) {
    console.log("[Electron] Headless mode active — UI window and tray icon skipped");
  } else if (startHidden) {
    console.log("[Electron] Launched hidden in background tray without a renderer");
  } else {
    showMainWindow();
  }`;
const newWindowDecision = `  if (isHeadless) {
    console.log("[Electron] Headless mode active — UI window and tray icon skipped");
  } else if (startHidden) {
    console.log("[Electron] Launched hidden in background tray without a renderer");
  } else if (serverReady) {
    if (mainWindow && !mainWindow.isDestroyed()) {
      lastRendererUrl = getServerUrl();
      mainWindow.loadURL(getServerUrl()).catch((error) => {
        const message = error instanceof Error ? error.message : String(error);
        console.error("[ABUKHALED AI] Dashboard navigation failed after startup:", message);
        void mainWindow && !mainWindow.isDestroyed() && recoverStartupDashboard(message);
      });
    }
  } else {
    console.warn("[ABUKHALED AI] Dashboard server is still initializing; branded startup screen remains visible.");
  }`;
// Use a named recovery helper in this lifecycle scope; the createWindow recovery
// closure is intentionally private to that window and is not reachable here.
const safeWindowDecision = newWindowDecision.replace(
  '        void mainWindow && !mainWindow.isDestroyed() && recoverStartupDashboard(message);',
  '        console.error("[ABUKHALED AI] Dashboard will retry through the startup readiness watcher:", message);'
);
main = replaceExactlyOnce(main, oldWindowDecision, safeWindowDecision, 'load dashboard after server readiness');
fs.writeFileSync(mainPath, main, 'utf8');

// The packaged smoke test must not pass merely because the local splash loaded.
let smoke = read(smokePath);
smoke = replaceExactlyOnce(
  smoke,
  'const rendererLoaded = logs.value.includes("[ABUKHALED AI] Renderer did-finish-load:");',
  'const rendererLoaded = logs.value.includes("[ABUKHALED AI] Dashboard renderer loaded:");',
  'packaged smoke test dashboard readiness marker'
);
smoke = replaceExactlyOnce(
  smoke,
  'const DEFAULT_TIMEOUT_MS = 90_000;',
  'const DEFAULT_TIMEOUT_MS = 180_000;',
  'packaged smoke test startup timeout'
);
fs.writeFileSync(smokePath, smoke, 'utf8');

// Persist the resource filter update after all validation has passed.
fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`, 'utf8');

// Final postconditions: exact source identity, expected lifecycle, packaged asset,
// and smoke-test semantics must all be present.
const finalMain = read(mainPath);
const finalSmoke = read(smokePath);
const finalPkg = JSON.parse(read(packagePath));
const finalReadiness = read(helperPath);
for (const marker of [
  'ABUKHALED_PATCH_EARLY_STARTUP_WINDOW_V1',
  'startupSplash = false',
  'window.loadFile(splashPath)',
  '[ABUKHALED AI] Startup splash loaded:',
  '[ABUKHALED AI] Dashboard renderer loaded:',
  'branded startup screen remains visible',
]) {
  if (!finalMain.includes(marker)) fail(`Electron startup verification failed: ${marker}`);
}
if (!finalReadiness.includes('/healthz') || finalReadiness.includes('/api/health/ping')) {
  fail('Final server-readiness verification failed.');
}
if (!finalSmoke.includes('[ABUKHALED AI] Dashboard renderer loaded:')) {
  fail('Packaged smoke test does not wait for the actual dashboard renderer.');
}
if (!finalSmoke.includes('const DEFAULT_TIMEOUT_MS = 180_000;')) {
  fail('Packaged smoke test timeout was not increased to 180 seconds.');
}
if (!finalPkg.build.extraResources.find((item) => item?.from === 'assets' && item?.to === 'assets')?.filter?.includes('startup.html')) {
  fail('Startup screen is not included in packaged resources.');
}
if (finalPkg.version !== '3.8.53' || finalPkg.build.appId !== 'online.omniroute.desktop') {
  fail('Upstream version or original application identity changed.');
}
if (!fs.existsSync(splashTargetPath)) fail('Packaged startup screen source is missing.');

console.log(`[${BRAND} startup patch] Lifecycle readiness endpoint verified: /healthz`);
console.log(`[${BRAND} startup patch] Branded startup screen is shown before server initialization.`);
console.log(`[${BRAND} startup patch] Dashboard navigation follows server readiness.`);
console.log(`[${BRAND} startup patch] Packaged smoke test waits for the actual dashboard renderer.`);
console.log(`[${BRAND} startup patch] OmniRoute ${finalPkg.version}; original appId preserved.`);
