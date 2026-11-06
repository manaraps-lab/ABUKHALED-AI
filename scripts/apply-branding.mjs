
/**
 * ABUKHALED AI branding layer
 * Pinned upstream: OmniRoute 3.8.53
 * Preserves the original appId and all user data paths.
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
  if (!fs.existsSync(file)) fail(`Required file not found: ${file}`);
  return fs.readFileSync(file, "utf8");
}

function replaceCount(source, before, after, expected, label) {
  const count = source.split(before).length - 1;

  if (count === 0 && source.includes(after)) return source;

  if (count !== expected) {
    fail(`${label}: expected ${expected} matches, found ${count}`);
  }

  return source.split(before).join(after);
}

function replaceFunctionBlock(source, startMarker, endMarker, replacement, label) {
  const start = source.indexOf(startMarker);

  if (start === -1 || source.indexOf(startMarker, start + 1) !== -1) {
    fail(`${label}: start marker missing or duplicated`);
  }

  const end = source.indexOf(endMarker, start + startMarker.length);

  if (end === -1) {
    fail(`${label}: end marker not found`);
  }

  return source.slice(0, start) + replacement + "\n" + source.slice(end);
}

// ---- Validate upstream package ----

const pkg = JSON.parse(read(packagePath));

if (pkg.version !== "3.8.53") {
  fail(`Unexpected Electron package version: ${pkg.version}`);
}

if (!pkg.build || pkg.build.appId !== "online.omniroute.desktop") {
  fail("Unexpected appId. Refusing to change the upstream data identity.");
}

pkg.build.productName = "ABUKHALED AI";
pkg.build.copyright = "Copyright © 2026 ABUKHALED AI";
delete pkg.build.publish;

fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);

// ---- Apply desktop branding ----

let main = read(mainPath);
const eol = main.includes("\r\n") ? "\r\n" : "\n";
main = main.replace(/\r\n/g, "\n");

// These are two DIFFERENT source strings and must be replaced separately.
main = replaceCount(
  main,
  'title: "OmniRoute",',
  'title: "ABUKHALED AI",',
  1,
  "desktop window title"
);

main = replaceCount(
  main,
  'title: "OmniRoute Update Ready",',
  'title: "ABUKHALED AI Update Ready",',
  1,
  "update notification title"
);

main = replaceCount(
  main,
  'label: "Open OmniRoute",',
  'label: "Open ABUKHALED AI",',
  1,
  "system tray menu label"
);

main = replaceCount(
  main,
  'tray.setToolTip("OmniRoute");',
  'tray.setToolTip("ABUKHALED AI");',
  1,
  "system tray tooltip"
);

// ---- Isolate upstream auto-update functions ----

main = replaceFunctionBlock(
  main,
  "async function checkForUpdates(silent = false) {",
  "async function downloadUpdate() {",
  `async function checkForUpdates(silent = false) {
  console.info("[ABUKHALED AI] Updates are disabled until the official release channel is configured.");
  sendToRenderer("update-status", {
    status: "not-available",
    message: "ABUKHALED AI updates are not configured yet."
  });
}`,
  "disable upstream auto-update checks"
);

main = replaceFunctionBlock(
  main,
  "async function downloadUpdate() {",
  "function installUpdate() {",
  `async function downloadUpdate() {
  throw new Error("ABUKHALED AI update channel is not configured.");
}`,
  "disable upstream update downloads"
);

main = replaceFunctionBlock(
  main,
  "function installUpdate() {",
  "// ── Content Security Policy",
  `function installUpdate() {
  console.warn("[ABUKHALED AI] Update installation is disabled until the official release channel is configured.");
}`,
  "disable upstream update installation"
);

// Prevent automatic installation on application exit.
main = replaceCount(
  main,
  "autoUpdater.autoInstallOnAppQuit = true;",
  "autoUpdater.autoInstallOnAppQuit = false;",
  1,
  "disable automatic update installation"
);

fs.writeFileSync(mainPath, main.replace(/\n/g, eol));

// ---- Verify final files ----

const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);

if (finalPkg.build.productName !== "ABUKHALED AI") {
  fail("productName verification failed");
}

if (finalPkg.build.appId !== "online.omniroute.desktop") {
  fail("appId changed unexpectedly");
}

if (Object.hasOwn(finalPkg.build, "publish")) {
  fail("Upstream publish configuration still exists");
}

for (const text of [
  'title: "ABUKHALED AI"',
  'title: "ABUKHALED AI Update Ready"',
  'label: "Open ABUKHALED AI"',
  'tray.setToolTip("ABUKHALED AI")'
]) {
  if (!finalMain.includes(text)) {
    fail(`Branding verification failed: ${text}`);
  }
}

if (!finalMain.includes("Updates are disabled until the official release channel is configured.")) {
  fail("Auto-update isolation verification failed");
}

if (!finalMain.includes("autoUpdater.autoInstallOnAppQuit = false;")) {
  fail("Automatic update installation is still enabled");
}

console.log("[ABUKHALED branding] Applied and verified.");
console.log(`Product: ${finalPkg.build.productName}`);
console.log(`Version: ${finalPkg.version}`);
console.log(`Preserved appId: ${finalPkg.build.appId}`);
console.log("Window title: verified");
console.log("Notification title: verified");
console.log("Tray branding: verified");
console.log("Upstream publishing: disabled");
console.log("Upstream auto-update checks/download/install: disabled");
