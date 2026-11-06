/**
 * Applies the first ABUKHALED AI desktop identity layer to the pinned OmniRoute
 * source tree. Intentionally preserves appId and all OmniRoute data-directory
 * paths during this first migration-safe phase.
 */
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(process.argv[2] || 'upstream');
const electronDir = path.join(root, 'electron');
const packagePath = path.join(electronDir, 'package.json');
const mainPath = path.join(electronDir, 'main.js');

function fail(message) {
  console.error(`[ABUKHALED branding] ERROR: ${message}`);
  process.exit(1);
}

function read(file) {
  if (!fs.existsSync(file)) fail(`Required source file not found: ${file}`);
  return fs.readFileSync(file, 'utf8');
}

function replaceExactly(source, before, after, label) {
  const count = source.split(before).length - 1;
  if (count === 0 && source.includes(after)) return source;
  if (count !== 1) fail(`${label}: expected exactly one source match, found ${count}`);
  return source.replace(before, after);
}

const pkg = JSON.parse(read(packagePath));

if (pkg.version !== '3.8.53') {
  fail(`Unexpected Electron package version: ${pkg.version}`);
}

if (!pkg.build || pkg.build.appId !== 'online.omniroute.desktop') {
  fail('Electron appId differs from the reviewed upstream baseline; stop for data-path review.');
}

pkg.build.productName = 'ABUKHALED AI';
pkg.build.copyright = 'Copyright © 2026 ABUKHALED AI';
delete pkg.build.publish;

fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);

let main = read(mainPath);
const mainEol = main.includes('\r\n') ? '\r\n' : '\n';
main = main.replace(/\r\n/g, '\n');

const replacements = [
  ['title: "OmniRoute",', 'title: "ABUKHALED AI",', 'window title'],
  ['label: "Open OmniRoute",', 'label: "Open ABUKHALED AI",', 'tray menu label'],
  ['tray.setToolTip("OmniRoute");', 'tray.setToolTip("ABUKHALED AI");', 'tray tooltip'],
  ['title: "OmniRoute Update Ready",', 'title: "ABUKHALED AI Update Ready",', 'update notification title'],
  ['"Name=OmniRoute",', '"Name=ABUKHALED AI",', 'desktop autostart name'],
  ['"Comment=OmniRoute Desktop Client",', '"Comment=ABUKHALED AI Desktop Client",', 'desktop autostart description'],
];

for (const [before, after, label] of replacements) {
  main = replaceExactly(main, before, after, label);
}

main = replaceExactly(
  main,
  'async function checkForUpdates(silent = false) {\n  if (isDev) {',
  'async function checkForUpdates(silent = false) {\n  console.info("[ABUKHALED AI] Updates are disabled until the official release channel is configured.");\n  sendToRenderer("update-status", { status: "not-available", message: "ABUKHALED AI updates are not configured yet." });\n  return;\n  if (isDev) {',
  'disable upstream auto-update checks'
);

main = replaceExactly(
  main,
  'async function downloadUpdate() {\n  await autoUpdater.downloadUpdate();\n}',
  'async function downloadUpdate() {\n  throw new Error("ABUKHALED AI update channel is not configured.");\n}',
  'guard upstream update downloads'
);

main = replaceExactly(
  main,
  'function installUpdate() {\n  if (nextServer) {',
  'function installUpdate() {\n  console.warn("[ABUKHALED AI] Update installation is disabled until the official release channel is configured.");\n  return;\n  if (nextServer) {',
  'guard upstream update installation'
);

fs.writeFileSync(mainPath, main.replace(/\n/g, mainEol));

const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);

if (finalPkg.build.productName !== 'ABUKHALED AI') {
  fail('productName postcondition failed');
}

if (finalPkg.build.appId !== 'online.omniroute.desktop') {
  fail('appId must remain unchanged in migration-safe phase');
}

if (Object.hasOwn(finalPkg.build, 'publish')) {
  fail('upstream publish configuration still exists');
}

for (const text of [
  'title: "ABUKHALED AI"',
  'label: "Open ABUKHALED AI"',
  'tray.setToolTip("ABUKHALED AI")'
]) {
  if (!finalMain.includes(text)) fail(`Branding postcondition failed: ${text}`);
}

if (!finalMain.includes('Updates are disabled until the official release channel is configured.')) {
  fail('Updater isolation postcondition failed');
}

console.log('[ABUKHALED branding] Applied and verified.');
console.log(`Product: ${finalPkg.build.productName}`);
console.log(`Version: ${finalPkg.version}`);
console.log(`Preserved appId: ${finalPkg.build.appId}`);
console.log('Upstream publish target: removed');
console.log('Upstream auto-update checks/download/install: disabled');
