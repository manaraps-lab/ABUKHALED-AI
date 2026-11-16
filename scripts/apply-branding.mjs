#!/usr/bin/env node
/**
 * ABUKHALED AI integration layer for the pinned OmniRoute 3.8.53 source.
 * Preserves the upstream engine, appId, data paths, translation semantics,
 * and existing desktop capabilities while applying the product identity.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(process.argv[2] || 'upstream');
const electronDir = path.join(root, 'electron');
const packagePath = path.join(electronDir, 'package.json');
const mainPath = path.join(electronDir, 'main.js');
const smokePath = path.join(root, 'scripts', 'dev', 'smoke-electron-packaged.mjs');
const layoutPath = path.join(root, 'src', 'app', 'layout.tsx');
const appConfigPath = path.join(root, 'src', 'shared', 'constants', 'appConfig.ts');
const sidebarPath = path.join(root, 'src', 'shared', 'components', 'Sidebar.tsx');
const headerPath = path.join(root, 'src', 'shared', 'components', 'Header.tsx');
const logoPath = path.join(root, 'src', 'shared', 'components', 'OmniRouteLogo.tsx');
const messagesDir = path.join(root, 'src', 'i18n', 'messages');
const assetsSourceDir = path.resolve(scriptDir, '..', 'assets');
const assetsTargetDir = path.join(electronDir, 'assets');
const publicDir = path.join(root, 'public');
const BRAND = 'ABUKHALED AI';

function fail(message) {
  console.error(`[ABUKHALED branding] ERROR: ${message}`);
  process.exit(1);
}
function read(file) {
  if (!fs.existsSync(file)) fail(`Required source file not found: ${file}`);
  return fs.readFileSync(file, 'utf8');
}
function write(file, value) {
  fs.writeFileSync(file, value, 'utf8');
}
function replaceLiteral(source, before, after, label) {
  const count = source.split(before).length - 1;
  if (count === 0 && source.includes(after)) return source;
  if (count !== 1) fail(`${label}: expected one source match (or already applied), found ${count}`);
  return source.replace(before, after);
}

// Replace a complete named function without relying on a fragile multiline regex.
function replaceFunction(source, name, replacement, isAsync = false) {
  const marker = `ABUKHALED_PATCH_${name.toUpperCase()}_V1`;
  if (source.includes(marker)) return source;
  const prefix = isAsync ? 'async function' : 'function';
  const declaration = new RegExp(`^${prefix}\\s+${name}\\s*\\([^\\n]*\\)\\s*\\{`, 'm');
  const match = declaration.exec(source);
  if (!match) fail(`Could not locate ${prefix} ${name}`);
  const start = match.index;
  const openBrace = start + match[0].lastIndexOf('{');
  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  const templateExpressionStack = [];
  for (let i = openBrace; i < source.length; i++) {
    const ch = source[i];
    const next = source[i + 1];
    if (lineComment) {
      if (ch === '\n') lineComment = false;
      continue;
    }
    if (blockComment) {
      if (ch === '*' && next === '/') { blockComment = false; i++; }
      continue;
    }
    if (quote) {
      if (escaped) { escaped = false; continue; }
      if (ch === '\\') { escaped = true; continue; }
      if (quote === '`' && ch === '$' && next === '{') {
        depth++;
        templateExpressionStack.push(depth);
        i++;
        quote = null;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '/' && next === '/') { lineComment = true; i++; continue; }
    if (ch === '/' && next === '*') { blockComment = true; i++; continue; }
    if (ch === '"' || ch === "'" || ch === '`') { quote = ch; continue; }
    if (ch === '{') { depth++; continue; }
    if (ch === '}') {
      depth--;
      if (templateExpressionStack.length > 0 && depth === templateExpressionStack[templateExpressionStack.length - 1] - 1) {
        templateExpressionStack.pop();
        quote = '`';
        continue;
      }
      if (depth === 0) return source.slice(0, start) + replacement + source.slice(i + 1);
    }
  }
  fail(`Could not find the closing brace of ${name}`);
}

function updateHomeDescription(messages, locale) {
  if (!messages || typeof messages !== 'object' || Array.isArray(messages)) {
    fail(`Invalid translation root for ${locale}`);
  }
  if (!messages.header || typeof messages.header !== 'object' || Array.isArray(messages.header)) {
    fail(`Missing header translation section for ${locale}`);
  }
  if (typeof messages.header.homeDescription !== 'string') {
    fail(`Missing header.homeDescription translation for ${locale}`);
  }

  const expected = locale === 'ar.json' ? 'مرحبًا بك في OmniRoute' : 'Welcome to OmniRoute';
  const branded = locale === 'ar.json' ? 'مرحبًا بك في ABUKHALED AI' : 'Welcome to ABUKHALED AI';
  const current = messages.header.homeDescription;
  if (current === branded) return messages;
  if (current !== expected) {
    fail(`Unexpected header.homeDescription value in ${locale}: ${current}`);
  }

  // Change only the exact welcome label. All other translation keys are preserved.
  messages.header.homeDescription = branded;
  return messages;
}

// Verify the immutable upstream baseline before changing anything.
const rootPackage = JSON.parse(read(path.join(root, 'package.json')));
const pkg = JSON.parse(read(packagePath));
if (rootPackage.version !== '3.8.53' || pkg.version !== '3.8.53') {
  fail(`Expected root and Electron version 3.8.53; found ${rootPackage.version}/${pkg.version}`);
}
if (pkg.build?.appId !== 'online.omniroute.desktop') {
  fail(`Unexpected appId: ${pkg.build?.appId ?? '(missing)'}`);
}

// Electron package identity. appId is intentionally retained for existing user data.
pkg.build.productName = BRAND;
pkg.build.copyright = 'Copyright © 2026 ABUKHALED AI';
delete pkg.build.publish;
write(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);

// Desktop identity, startup recovery, and isolation from the upstream update feed.
let main = read(mainPath).replace(/\r\n?/g, '\n');
main = replaceLiteral(main, 'title: "OmniRoute",', 'title: "ABUKHALED AI",', 'window title');
main = replaceLiteral(main, 'label: "Open OmniRoute",', 'label: "Open ABUKHALED AI",', 'tray menu label');
main = replaceLiteral(main, 'tray.setToolTip("OmniRoute");', 'tray.setToolTip("ABUKHALED AI");', 'tray tooltip');
main = replaceLiteral(main, 'title: "OmniRoute Update Ready",', 'title: "ABUKHALED AI Update Ready",', 'update notification title');
main = replaceLiteral(main, '"Name=OmniRoute",', '"Name=ABUKHALED AI",', 'Linux autostart name');
main = replaceLiteral(main, '"Comment=OmniRoute Desktop Client",', '"Comment=ABUKHALED AI Desktop Client",', 'Linux autostart comment');
if (!main.includes('ABUKHALED_PATCH_PRESERVE_USER_DATA_V1')) {
  main = replaceLiteral(
    main,
    'const gotTheLock = app.requestSingleInstanceLock();',
    '// ABUKHALED_PATCH_PRESERVE_USER_DATA_V1\n' +
      'const legacyUserDataPath = path.join(app.getPath("appData"), "OmniRoute");\n' +
      'app.setPath("userData", legacyUserDataPath);\n' +
      'const gotTheLock = app.requestSingleInstanceLock();',
    'preserve the existing Electron userData directory'
  );
}

main = replaceFunction(main, 'createWindow', `function createWindow({ showWhenReady = true } = {}) {
  if (mainWindow && !mainWindow.isDestroyed()) return mainWindow;

  const rendererStartedAt = Date.now();
  const platformWindowOptions =
    process.platform === "darwin"
      ? { titleBarStyle: "hiddenInset", trafficLightPosition: { x: 16, y: 16 } }
      : { titleBarStyle: "default" };
  const window = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 1024,
    minHeight: 700,
    title: "ABUKHALED AI",
    icon: path.join(RESOURCES_PATH, "assets", "icon.png"),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      webSecurity: true,
      webviewTag: false,
    },
    show: false,
    backgroundColor: "#0a0a0a",
    ...platformWindowOptions,
  });
  mainWindow = window;

  // ABUKHALED_PATCH_CREATEWINDOW_V1
  let recoveryAttempts = 0;
  let recoveryInProgress = false;
  let rendererCrashRecoveryUsed = false;
  let startupWindowShown = false;
  let rendererReadyToShow = false;

  const showStartupWindow = () => {
    if (!showWhenReady || startupWindowShown || window.isDestroyed()) return;
    startupWindowShown = true;
    window.show();
    console.log("[ABUKHALED AI] Main window shown.");
  };

  const recoverRenderer = async (reason) => {
    if (window.isDestroyed() || recoveryInProgress || recoveryAttempts >= 2) return;
    recoveryInProgress = true;
    try {
      while (recoveryAttempts < 2 && !window.isDestroyed()) {
        recoveryAttempts += 1;
        console.warn(
          "[ABUKHALED AI] Renderer recovery attempt " + recoveryAttempts + ": " + reason
        );
        const serverReady = isDev || await waitForServer(getServerReadinessUrl(), 30000);
        if (!serverReady) throw new Error("The dashboard server did not become ready during renderer recovery.");
        await new Promise((resolve) => setTimeout(resolve, 500 * recoveryAttempts));
        if (window.isDestroyed()) return;
        try {
          const targetUrl = resolveRendererUrl(lastRendererUrl, getServerUrl());
          await window.loadURL(targetUrl);
          console.info("[ABUKHALED AI] Renderer navigation recovered:", targetUrl);
          return;
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          console.warn("[ABUKHALED AI] Renderer recovery navigation failed:", message);
        }
      }
      console.error("[ABUKHALED AI] Renderer recovery exhausted; dashboard remains unavailable.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error("[ABUKHALED AI] Renderer recovery failed:", message);
    } finally {
      recoveryInProgress = false;
    }
  };

  window.webContents.on("did-fail-load", (_event, errorCode, errorDescription, validatedURL, isMainFrame) => {
    if (!isMainFrame || errorCode === -3) return;
    console.error(
      "[ABUKHALED AI] Main-frame load failed:",
      errorCode,
      errorDescription,
      validatedURL
    );
    void recoverRenderer(errorDescription || String(errorCode));
  });

  window.webContents.on("did-finish-load", () => {
    console.log("[ABUKHALED AI] Renderer did-finish-load:", window.webContents.getURL());
    recoveryAttempts = 0;
    if (rendererReadyToShow) showStartupWindow();
  });

  window.webContents.on("render-process-gone", (_event, details) => {
    console.error("[ABUKHALED AI] Renderer process terminated:", details);
    if (!rendererCrashRecoveryUsed && (details.reason === "crashed" || details.reason === "oom")) {
      rendererCrashRecoveryUsed = true;
      void recoverRenderer("renderer process " + details.reason);
    }
  });

  window.webContents.on("console-message", (_event, level, message, line, sourceId) => {
    if (level >= 2) {
      console.warn("[ABUKHALED AI] Renderer console error:", message, "at", sourceId + ":" + line);
    }
  });

  window.on("unresponsive", () => {
    console.warn("[ABUKHALED AI] Renderer window became unresponsive.");
  });

  const initialUrl = resolveRendererUrl(lastRendererUrl, getServerUrl());
  window.loadURL(initialUrl).catch((error) => {
    const message = error instanceof Error ? error.message : String(error);
    console.error("[ABUKHALED AI] Initial renderer navigation failed:", message);
    void recoverRenderer(message);
  });

  if (isDev) window.webContents.openDevTools({ mode: "detach" });

  window.once("ready-to-show", () => {
    rendererReadyToShow = true;
    console.log("[Electron] Renderer ready in " + (Date.now() - rendererStartedAt) + "ms");
    if (showWhenReady) {
      showStartupWindow();
    } else {
      console.log("[Electron] Launched hidden in background tray");
    }
  });

  window.webContents.setWindowOpenHandler(({ url }) => {
    try {
      const parsedUrl = new URL(url);
      if (["http:", "https:"].includes(parsedUrl.protocol)) {
        shell.openExternal(url);
      } else {
        console.warn("[Electron] Blocked unsafe protocol:", parsedUrl.protocol);
      }
    } catch {
      console.error("[Electron] Blocked invalid URL:", url);
    }
    return { action: "deny" };
  });

  window.on("close", (event) => {
    if (!app.isQuitting) {
      event.preventDefault();
      lastRendererUrl = resolveRendererUrl(window.webContents.getURL(), getServerUrl());
      if (closeBehavior === CLOSE_BEHAVIOR_UNLOAD) {
        console.log("[Electron] Dashboard renderer unloaded; server remains running");
        window.destroy();
      } else {
        console.log("[Electron] Dashboard hidden; renderer kept loaded");
        window.hide();
      }
    }
    return false;
  });

  window.on("closed", () => {
    if (mainWindow === window) mainWindow = null;
  });
  return window;
}`, false);

main = replaceFunction(main, 'checkForUpdates', `async function checkForUpdates(silent = false) {
  console.info("[ABUKHALED AI] Upstream auto-updates are disabled; ABUKHALED AI manages its own release channel.");
  if (!silent) {
    sendToRenderer("update-status", {
      status: "not-available",
      message: "ABUKHALED AI updates are managed separately.",
    });
  }
  return;
}`, true);
main = replaceFunction(main, 'downloadUpdate', `async function downloadUpdate() {
  throw new Error("ABUKHALED AI update downloads are not configured in this build.");
}`, true);
main = replaceFunction(main, 'installUpdate', `function installUpdate() {
  console.warn("[ABUKHALED AI] Upstream update installation is disabled.");
  return;
}`, false);
write(mainPath, main);

// Strengthen the packaged smoke test: a live HTTP endpoint is not enough;
// Electron must also finish loading the renderer and show its main window.
let smoke = read(smokePath).replace(/\r\n?/g, '\n');
smoke = replaceLiteral(smoke, 'const DEFAULT_TIMEOUT_MS = 45_000;', 'const DEFAULT_TIMEOUT_MS = 90_000;', 'packaged renderer smoke timeout');
smoke = replaceFunction(smoke, 'waitForReady', `async function waitForReady({ logs, smokeUrl, timeoutMs, settleMs, exitState }) {
  // ABUKHALED_PATCH_WAITFORREADY_V1
  const startedAt = Date.now();
  let lastError = null;

  while (Date.now() - startedAt < timeoutMs) {
    assertNoFatalLogs(logs.value);
    if (exitState.spawnError !== null) {
      throw new Error("Packaged Electron app failed to launch: " + exitState.spawnError.message);
    }
    if (exitState.exitCode !== null || exitState.signalCode !== null) {
      throw new Error("Packaged Electron app exited before readiness: code=" + exitState.exitCode + " signal=" + exitState.signalCode);
    }
    try {
      const response = await fetchWithTimeout(smokeUrl, 1_000);
      const rendererLoaded = logs.value.includes("[ABUKHALED AI] Renderer did-finish-load:");
      const windowShown = logs.value.includes("[ABUKHALED AI] Main window shown.");
      if (response.status === 200 && rendererLoaded && windowShown) {
        assertNoFatalLogs(logs.value);
        console.log("[electron-smoke] server ready: " + smokeUrl + " returned HTTP 200");
        console.log("[electron-smoke] Electron renderer loaded and main window shown");
        await settleAfterReady({
          getExitState: () => ({ exitCode: exitState.exitCode, signalCode: exitState.signalCode }),
          logs,
          settleMs,
        });
        console.log("[electron-smoke] stable for " + settleMs + "ms after renderer readiness");
        return;
      }
      lastError = new Error(
        response.status !== 200
          ? "HTTP " + response.status
          : "HTTP 200 received; waiting for Electron renderer load and visible main window logs"
      );
    } catch (error) {
      lastError = error;
    }
    await sleep(500);
  }

  throw new Error(
    "Packaged Electron app did not complete server + renderer readiness within " + timeoutMs + "ms. Last error: " +
      (lastError instanceof Error ? lastError.message : String(lastError))
  );
}`, true);
write(smokePath, smoke);

// Dashboard metadata and application identity.
let layout = read(layoutPath).replace(/\r\n?/g, '\n');
layout = replaceLiteral(layout, 'const instanceName = settings.instanceName;', 'const instanceName = "ABUKHALED AI";', 'dashboard metadata application name');
layout = replaceLiteral(
  layout,
  '"OmniRoute is an AI gateway for multi-provider LLMs. One endpoint for all your AI providers.",',
  '"ABUKHALED AI is an AI gateway for multi-provider LLMs. One endpoint for your AI providers.",',
  'dashboard metadata description'
);
write(layoutPath, layout);

let appConfig = read(appConfigPath).replace(/\r\n?/g, '\n');
appConfig = replaceLiteral(appConfig, 'name: "OmniRoute",', 'name: "ABUKHALED AI",', 'application default name');
write(appConfigPath, appConfig);

let sidebar = read(sidebarPath).replace(/\r\n?/g, '\n');
sidebar = replaceLiteral(sidebar, 'const brandName = customAppName || APP_CONFIG.name;', 'const brandName = APP_CONFIG.name;', 'sidebar fixed brand name');
sidebar = replaceLiteral(
  sidebar,
  '<OmniRouteLockup\n                  name={customAppName}',
  '<OmniRouteLockup\n                  name={brandName}',
  'sidebar lockup name'
);
write(sidebarPath, sidebar);

let header = read(headerPath).replace(/\r\n?/g, '\n');
header = replaceLiteral(header, 'import { ProductSwitcher } from "@omniroute/ui-kit/keak-brand";\n', '', 'Keak product switcher import');
header = replaceLiteral(header, 'import { KEAK_ACTIVE_PRODUCT, KEAK_PRODUCT_LINKS } from "@/shared/constants/keakProducts";\n', '', 'Keak product links import');
header = replaceLiteral(
  header,
  `        <ProductSwitcher\n          items={KEAK_PRODUCT_LINKS}\n          activeProduct={KEAK_ACTIVE_PRODUCT}\n          className="hidden md:block"\n          aria-label={t("products")}\n        />\n`,
  `        <span className="hidden md:inline-flex items-center text-sm font-semibold text-text-main">\n          ABUKHALED AI\n        </span>\n`,
  'Keak product switcher UI'
);
write(headerPath, header);

const logoSource = `"use client";\n\nimport { cn } from "@/shared/utils/cn";\n\ntype OmniRouteLogoProps = { size?: number; className?: string };\n\nexport default function OmniRouteLogo({ size = 20, className = "" }: OmniRouteLogoProps) {\n  return (\n    <img src="/abukhaled-icon.png" alt="" width={size} height={size} aria-hidden="true" className={cn("object-contain", className)} />\n  );\n}\n\ninterface OmniRouteLockupProps { name?: string | null; logoSrc?: string | null; className?: string }\n\nexport function OmniRouteLockup({ name, logoSrc, className }: OmniRouteLockupProps) {\n  return (\n    <span role="img" aria-label={name || "ABUKHALED AI"} className={cn("inline-flex items-center gap-2", className)}>\n      <img src={logoSrc || "/abukhaled-icon.png"} alt="" className="size-6 object-contain" />\n      <span aria-hidden="true" className="truncate font-semibold">{name || "ABUKHALED AI"}</span>\n    </span>\n  );\n}\n`;
const previousLogo = read(logoPath);
if (!previousLogo.includes('ProductIcon') && !previousLogo.includes('/abukhaled-icon.png')) {
  fail('Unexpected logo component source; refusing to replace it.');
}
write(logoPath, logoSource);

// Only change the user-facing welcome label. Keep technical engine references,
// API examples, documentation, integration instructions and legal provenance intact.
if (!fs.existsSync(messagesDir)) fail(`Translation directory not found: ${messagesDir}`);
const messageFiles = ['ar.json', 'en.json'];
for (const filename of messageFiles) {
  if (!fs.existsSync(path.join(messagesDir, filename))) fail(`Required translation file not found: ${filename}`);
}
const translationBaselines = new Map();
for (const filename of messageFiles) {
  const file = path.join(messagesDir, filename);
  const messages = JSON.parse(read(file));
  translationBaselines.set(filename, JSON.parse(JSON.stringify(messages)));
  const updated = updateHomeDescription(messages, filename);
  write(file, `${JSON.stringify(updated, null, 2)}\n`);
}

// Copy branded desktop and dashboard icons.
fs.mkdirSync(assetsTargetDir, { recursive: true });
fs.mkdirSync(publicDir, { recursive: true });
for (const filename of ['icon.ico', 'icon.png', 'tray-icon.png']) {
  const source = path.join(assetsSourceDir, filename);
  if (!fs.existsSync(source)) fail(`Branding asset is missing: ${source}`);
  fs.copyFileSync(source, path.join(assetsTargetDir, filename));
}
fs.copyFileSync(path.join(assetsSourceDir, 'icon.png'), path.join(publicDir, 'abukhaled-icon.png'));

// Postconditions: fail the build if identity, recovery, smoke checks or data safety drift.
const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);
const finalSmoke = read(smokePath);
const finalLayout = read(layoutPath);
const finalAppConfig = read(appConfigPath);
const finalSidebar = read(sidebarPath);
const finalHeader = read(headerPath);
const finalLogo = read(logoPath);
if (finalPkg.version !== '3.8.53') fail('Electron version changed unexpectedly');
if (finalPkg.build?.appId !== 'online.omniroute.desktop') fail('Original appId was not preserved');
if (!finalMain.includes('app.setPath("userData", legacyUserDataPath);')) fail('Existing Electron userData directory was not preserved');
if (finalPkg.build?.productName !== BRAND) fail('Product branding verification failed');
if (Object.hasOwn(finalPkg.build || {}, 'publish')) fail('Upstream publish configuration remains enabled');
for (const marker of [
  'ABUKHALED_PATCH_CREATEWINDOW_V1',
  'ABUKHALED_PATCH_PRESERVE_USER_DATA_V1',
  '[ABUKHALED AI] Renderer did-finish-load:',
  '[ABUKHALED AI] Main window shown.',
  '[ABUKHALED AI] Main-frame load failed:',
  'Upstream auto-updates are disabled',
  'ABUKHALED AI update downloads are not configured',
  'Upstream update installation is disabled',
]) {
  if (!finalMain.includes(marker)) fail(`Electron verification failed: ${marker}`);
}
if (!finalSmoke.includes('const DEFAULT_TIMEOUT_MS = 90_000;')) fail('Packaged renderer smoke timeout was not increased to 90 seconds');
if (!finalSmoke.includes('waiting for Electron renderer load and visible main window logs')) {
  fail('Packaged smoke test does not verify Electron renderer readiness');
}
if (!finalLayout.includes('const instanceName = "ABUKHALED AI";')) fail('Dashboard metadata branding failed');
if (!finalLayout.includes('ABUKHALED AI is an AI gateway')) fail('Dashboard description branding failed');
if (!finalAppConfig.includes('name: "ABUKHALED AI",')) fail('Application default name verification failed');
if (!finalSidebar.includes('const brandName = APP_CONFIG.name;') || !finalSidebar.includes('name={brandName}')) {
  fail('Sidebar brand verification failed');
}
if (finalHeader.includes('ProductSwitcher') || finalHeader.includes('KEAK_PRODUCT_LINKS')) {
  fail('Keak product switcher remains in dashboard header');
}
if (!finalHeader.includes('ABUKHALED AI')) fail('Dashboard header brand verification failed');
if (!finalLogo.includes('/abukhaled-icon.png')) fail('ABUKHALED logo asset is not wired into the logo component');
if (!fs.existsSync(path.join(publicDir, 'abukhaled-icon.png'))) fail('Dashboard icon was not copied');
for (const locale of messageFiles) {
  const messages = JSON.parse(read(path.join(messagesDir, locale)));
  const baseline = translationBaselines.get(locale);
  const expectedWelcome = locale === 'ar.json' ? 'مرحبًا بك في ABUKHALED AI' : 'Welcome to ABUKHALED AI';
  if (!messages.header || messages.header.homeDescription !== expectedWelcome) {
    fail(`Welcome translation verification failed for ${locale}`);
  }
  const expectedMessages = JSON.parse(JSON.stringify(baseline));
  expectedMessages.header.homeDescription = expectedWelcome;
  if (JSON.stringify(messages) !== JSON.stringify(expectedMessages)) {
    fail(`Unexpected translation changes detected outside header.homeDescription in ${locale}`);
  }
}
console.log('[ABUKHALED branding] Identity, renderer recovery and packaged smoke checks applied.');
console.log('[ABUKHALED branding] OmniRoute 3.8.53 source and original appId preserved.');
console.log('[ABUKHALED branding] Technical OmniRoute references in translations were retained.');
console.log('[ABUKHALED branding] Packaged smoke test now requires renderer load and visible window.');
console.log(`[ABUKHALED branding] Updated ${messageFiles.length} translation files safely.`);
