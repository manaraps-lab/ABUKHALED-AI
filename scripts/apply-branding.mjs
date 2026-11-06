/**
 * ABUKHALED AI branding integration.
 * Pinned upstream: OmniRoute 3.8.53
 *
 * Safety:
 * - Preserve upstream appId and user-data paths.
 * - Preserve upstream application version.
 * - Do not publish to upstream release channels.
 * - Disable upstream update operations until an official ABUKHALED
 *   release channel is implemented and verified.
 */

import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] || "upstream");
const electronDir = path.join(root, "electron");
const packagePath = path.join(electronDir, "package.json");
const mainPath = path.join(electronDir, "main.js");

function fail(message) {
  console.error(`[ABUKHALED branding] ERROR: ${message}`);
  process.exit(1);
}

function read(file) {
  if (!fs.existsSync(file)) {
    fail(`Required file not found: ${file}`);
  }

  return fs.readFileSync(file, "utf8");
}

function replaceCount(source, before, after, expected, label) {
  const count = source.split(before).length - 1;

  if (count !== expected) {
    fail(`${label}: expected ${expected} matches, found ${count}`);
  }

  return source.split(before).join(after);
}

/*
 * Replace a complete function using the next function's declaration
 * as its boundary. This does not depend on comments or line endings
 * between the two functions.
 */
function replaceFunctionBlock(source, startMarker, endMarker, replacement, label) {
  const start = source.indexOf(startMarker);

  if (start === -1) {
    fail(`${label}: start marker not found`);
  }

  const secondStart = source.indexOf(startMarker, start + startMarker.length);

  if (secondStart !== -1) {
    fail(`${label}: start marker occurs more than once`);
  }

  const end = source.indexOf(endMarker, start + startMarker.length);

  if (end === -1) {
    fail(`${label}: end marker not found`);
  }

  if (end <= start) {
    fail(`${label}: invalid function boundaries`);
  }

  return (
    source.slice(0, start) +
    replacement +
    "\n" +
    source.slice(end)
  );
}

/* ---------------------------------------------------------------
 * 1. Validate upstream package metadata.
 * ------------------------------------------------------------- */

if (!fs.existsSync(electronDir)) {
  fail(`Electron directory not found: ${electronDir}`);
}

const pkg = JSON.parse(read(packagePath));

if (pkg.version !== "3.8.53") {
  fail(`Unexpected Electron version: ${pkg.version}`);
}

if (!pkg.build) {
  fail("Electron build configuration is missing.");
}

if (pkg.build.appId !== "online.omniroute.desktop") {
  fail("Unexpected upstream appId. Refusing to risk changing the data identity.");
}

if (!pkg.build.win || !Array.isArray(pkg.build.win.target)) {
  fail("Unexpected Windows packaging configuration.");
}

if (!pkg.build.nsis) {
  fail("NSIS configuration is missing.");
}

/* ---------------------------------------------------------------
 * 2. Apply product identity without changing appId or version.
 * ------------------------------------------------------------- */

pkg.build.productName = "ABUKHALED AI";
pkg.build.copyright = "Copyright © 2026 ABUKHALED AI";

/*
 * Remove upstream publishing configuration.
 * No new publishing destination is configured at this stage.
 */
delete pkg.build.publish;

fs.writeFileSync(
  packagePath,
  `${JSON.stringify(pkg, null, 2)}\n`,
  "utf8"
);

/* ---------------------------------------------------------------
 * 3. Read main process and normalize line endings.
 * ------------------------------------------------------------- */

let main = read(mainPath);

const originalEol = main.includes("\r\n") ? "\r\n" : "\n";

main = main.replace(/\r\n/g, "\n");

/* ---------------------------------------------------------------
 * 4. Apply desktop branding.
 *
 * The upstream source contains two title declarations:
 * - Update notification title
 * - Main BrowserWindow title
 * ------------------------------------------------------------- */

main = replaceCount(
  main,
  'title: "OmniRoute",',
  'title: "ABUKHALED AI",',
  2,
  "desktop and notification titles"
);

main = replaceCount(
  main,
  'label: "Open OmniRoute",',
  'label: "Open ABUKHALED AI",',
  1,
  "tray menu label"
);

main = replaceCount(
  main,
  'tray.setToolTip("OmniRoute");',
  'tray.setToolTip("ABUKHALED AI");',
  1,
  "tray tooltip"
);

/* ---------------------------------------------------------------
 * 5. Disable upstream auto-update operations.
 *
 * The markers below match the actual function order in the
 * approved upstream main.js source.
 * ------------------------------------------------------------- */

main = replaceFunctionBlock(
  main,
  "async function checkForUpdates(silent = false) {",
  "async function downloadUpdate() {",
  `async function checkForUpdates(silent = false) {
  console.info(
    "[ABUKHALED AI] Automatic updates are disabled until the official release channel is configured."
  );

  if (!silent) {
    sendToRenderer("update-status", {
      status: "not-available",
      message: "ABUKHALED AI updates are not configured yet."
    });
  }

  return;
}`,
  "disable upstream auto-update checks"
);

main = replaceFunctionBlock(
  main,
  "async function downloadUpdate() {",
  "function installUpdate() {",
  `async function downloadUpdate() {
  throw new Error(
    "ABUKHALED AI update channel is not configured."
  );
}`,
  "disable upstream update downloads"
);

main = replaceFunctionBlock(
  main,
  "function installUpdate() {",
  "// ── Content Security Policy",
  `function installUpdate() {
  console.warn(
    "[ABUKHALED AI] Update installation is disabled until the official release channel is configured."
  );

  return;
}`,
  "disable upstream update installation"
);

/*
 * Prevent automatic installation on application exit as well.
 * This setting is defined near the updater configuration.
 */
main = replaceCount(
  main,
  "autoUpdater.autoInstallOnAppQuit = true;",
  "autoUpdater.autoInstallOnAppQuit = false;",
  1,
  "disable automatic update installation on quit"
);

/* ---------------------------------------------------------------
 * 6. Write main.js using its original line-ending convention.
 * ------------------------------------------------------------- */

const outputMain =
  originalEol === "\r\n"
    ? main.replace(/\n/g, "\r\n")
    : main;

fs.writeFileSync(mainPath, outputMain, "utf8");

/* ---------------------------------------------------------------
 * 7. Verify all postconditions.
 * ------------------------------------------------------------- */

const finalPkg = JSON.parse(read(packagePath));
let finalMain = read(mainPath);

finalMain = finalMain.replace(/\r\n/g, "\n");

if (finalPkg.version !== "3.8.53") {
  fail("Version verification failed.");
}

if (finalPkg.build.productName !== "ABUKHALED AI") {
  fail("Product name verification failed.");
}

if (finalPkg.build.appId !== "online.omniroute.desktop") {
  fail("Original appId was not preserved.");
}

if (Object.hasOwn(finalPkg.build, "publish")) {
  fail("Upstream publishing configuration is still present.");
}

if (
  finalMain.split('title: "ABUKHALED AI",').length - 1 !== 2
) {
  fail("Desktop title verification failed.");
}

if (!finalMain.includes('label: "Open ABUKHALED AI",')) {
  fail("Tray menu branding verification failed.");
}

if (!finalMain.includes('tray.setToolTip("ABUKHALED AI");')) {
  fail("Tray tooltip verification failed.");
}

if (
  !finalMain.includes(
    "Automatic updates are disabled until the official release channel is configured."
  )
) {
  fail("Auto-update check guard verification failed.");
}

if (
  !finalMain.includes(
    "ABUKHALED AI update channel is not configured."
  )
) {
  fail("Update download guard verification failed.");
}

if (
  !finalMain.includes(
    "Update installation is disabled until the official release channel is configured."
  )
) {
  fail("Update installation guard verification failed.");
}

if (!finalMain.includes("autoUpdater.autoInstallOnAppQuit = false;")) {
  fail("Automatic update installation on quit is not disabled.");
}

/* ---------------------------------------------------------------
 * 8. Report verified changes.
 * ------------------------------------------------------------- */

console.log("");
console.log("==============================================");
console.log("[ABUKHALED branding] Applied and verified.");
console.log("==============================================");
console.log(`Product name: ${finalPkg.build.productName}`);
console.log(`Version: ${finalPkg.version}`);
console.log(`Preserved appId: ${finalPkg.build.appId}`);
console.log("Upstream publishing: disabled");
console.log("Automatic update checks: disabled");
console.log("Update downloads: disabled");
console.log("Update installation: disabled");
console.log("Automatic installation on quit: disabled");
console.log("Desktop title and tray branding: verified");
console.log("==============================================");
