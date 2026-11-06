/**
 * ABUKHALED AI branding integration for the pinned OmniRoute 3.8.53 source.
 *
 * Migration-safe rules:
 * - Preserve the original Electron appId.
 * - Preserve all upstream data-directory paths.
 * - Preserve the upstream version.
 * - Do not publish to the upstream GitHub release channel.
 * - Disable upstream update operations until an official ABUKHALED channel exists.
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
    fail(`Required source file not found: ${file}`);
  }

  return fs.readFileSync(file, "utf8");
}

function writeJson(file, value) {
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function replaceExactly(source, before, after, label) {
  const count = source.split(before).length - 1;

  if (count === 0 && source.includes(after)) {
    return source;
  }

  if (count !== 1) {
    fail(`${label}: expected exactly one source match, found ${count}`);
  }

  return source.replace(before, after);
}

function replaceFunction(source, expression, replacement, label) {
  const matches = [...source.matchAll(expression)];

  if (matches.length !== 1) {
    fail(`${label}: expected exactly one function block, found ${matches.length}`);
  }

  return source.replace(expression, replacement);
}

if (!fs.existsSync(electronDir)) {
  fail(`Electron source directory not found: ${electronDir}`);
}

const pkg = JSON.parse(read(packagePath));

if (pkg.version !== "3.8.53") {
  fail(`Unexpected Electron package version: ${pkg.version}`);
}

if (!pkg.build || pkg.build.appId !== "online.omniroute.desktop") {
  fail("Electron appId differs from the reviewed upstream baseline; stop for data-path review.");
}

if (!pkg.build.win || !Array.isArray(pkg.build.win.target)) {
  fail("Unexpected Windows packaging configuration.");
}

if (!pkg.build.nsis || !pkg.build.nsis.artifactName) {
  fail("Unexpected NSIS installer configuration.");
}

/*
 * Apply product identity while retaining the original appId and paths.
 */
pkg.build.productName = "ABUKHALED AI";
pkg.build.copyright = "Copyright © 2026 ABUKHALED AI";

/*
 * Remove the upstream publishing destination.
 * Do not configure a new release destination until it exists and is verified.
 */
delete pkg.build.publish;

writeJson(packagePath, pkg);

/*
 * Normalize line endings before source transformations.
 * Restore the original line-ending style when writing the file.
 */
let main = read(mainPath);
const originalEol = main.includes("\r\n") ? "\r\n" : "\n";

main = main.replace(/\r\n/g, "\n");

/*
 * Desktop identity strings.
 */
const brandingReplacements = [
  [
    'title: "OmniRoute",',
    'title: "ABUKHALED AI",',
    "window title",
  ],
  [
    'label: "Open OmniRoute",',
    'label: "Open ABUKHALED AI",',
    "tray menu label",
  ],
  [
    'tray.setToolTip("OmniRoute");',
    'tray.setToolTip("ABUKHALED AI");',
    "tray tooltip",
  ],
  [
    'title: "OmniRoute Update Ready",',
    'title: "ABUKHALED AI Update Ready",',
    "update notification title",
  ],
  [
    '"Name=OmniRoute",',
    '"Name=ABUKHALED AI",',
    "desktop autostart name",
  ],
  [
    '"Comment=OmniRoute Desktop Client",',
    '"Comment=ABUKHALED AI Desktop Client",',
    "desktop autostart description",
  ],
];

for (const [before, after, label] of brandingReplacements) {
  main = replaceExactly(main, before, after, label);
}

/*
 * Replace complete updater functions.
 *
 * This avoids inserting early returns into the original functions,
 * leaving unreachable code, or relying on fragile line-ending matches.
 */
main = replaceFunction(
  main,
  /async function checkForUpdates\(silent = false\) \{[\s\S]*?\n\}(?=\nasync function downloadUpdate\(\))/g,
  `async function checkForUpdates(silent = false) {
  console.info("[ABUKHALED AI] Updates are disabled until the official release channel is configured.");

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

main = replaceFunction(
  main,
  /async function downloadUpdate\(\) \{[\s\S]*?\n\}(?=\nfunction installUpdate\(\))/g,
  `async function downloadUpdate() {
  throw new Error("ABUKHALED AI update channel is not configured.");
}`,
  "guard upstream update downloads"
);

main = replaceFunction(
  main,
  /function installUpdate\(\) \{[\s\S]*?\n\}(?=\n\/\/ ── Content Security Policy)/g,
  `function installUpdate() {
  console.warn("[ABUKHALED AI] Update installation is disabled until the official release channel is configured.");
  return;
}`,
  "guard upstream update installation"
);

/*
 * Restore original line-ending style and save.
 */
fs.writeFileSync(
  mainPath,
  originalEol === "\r\n" ? main.replace(/\n/g, "\r\n") : main,
  "utf8"
);

/*
 * Post-change verification.
 */
const finalPkg = JSON.parse(read(packagePath));
let finalMain = read(mainPath);

finalMain = finalMain.replace(/\r\n/g, "\n");

if (finalPkg.version !== "3.8.53") {
  fail("Version postcondition failed.");
}

if (finalPkg.build.productName !== "ABUKHALED AI") {
  fail("productName postcondition failed.");
}

if (finalPkg.build.appId !== "online.omniroute.desktop") {
  fail("appId must remain unchanged in migration-safe phase.");
}

if (Object.hasOwn(finalPkg.build, "publish")) {
  fail("Upstream publish configuration still exists.");
}

for (const text of [
  'title: "ABUKHALED AI",',
  'label: "Open ABUKHALED AI",',
  'tray.setToolTip("ABUKHALED AI");',
]) {
  if (!finalMain.includes(text)) {
    fail(`Branding postcondition failed: ${text}`);
  }
}

if (
  !finalMain.includes(
    "Updates are disabled until the official release channel is configured."
  )
) {
  fail("Updater isolation postcondition failed.");
}

if (
  !finalMain.includes(
    'throw new Error("ABUKHALED AI update channel is not configured.");'
  )
) {
  fail("Update download guard postcondition failed.");
}

if (
  !finalMain.includes(
    "Update installation is disabled until the official release channel is configured."
  )
) {
  fail("Update installation guard postcondition failed.");
}

console.log("[ABUKHALED branding] Applied and verified.");
console.log(`Product: ${finalPkg.build.productName}`);
console.log(`Version: ${finalPkg.version}`);
console.log(`Preserved appId: ${finalPkg.build.appId}`);
console.log("Upstream publish target: removed");
console.log("Upstream auto-update checks/download/install: disabled");
