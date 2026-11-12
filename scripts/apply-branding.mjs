
#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(process.argv[2] || 'upstream');
const electronDir = path.join(root, 'electron');
const packagePath = path.join(electronDir, 'package.json');
const mainPath = path.join(electronDir, 'main.js');
const assetsSourceDir = path.resolve(scriptDir, '..', 'assets');
const assetsTargetDir = path.join(electronDir, 'assets');

function fail(message) {
  console.error(`[ABUKHALED branding] ERROR: ${message}`);
  process.exit(1);
}

function read(file) {
  if (!fs.existsSync(file)) {
    fail(`Required source file not found: ${file}`);
  }
  return fs.readFileSync(file, 'utf8');
}

function write(file, value) {
  fs.writeFileSync(file, value, 'utf8');
}

function replaceLiteral(source, before, after, label) {
  const count = source.split(before).length - 1;

  if (count === 0 && source.includes(after)) {
    return source;
  }

  if (count !== 1) {
    fail(`${label}: expected one source match (or an already branded value), found ${count}`);
  }

  return source.replace(before, after);
}

function replaceTopLevelFunction(source, name, replacement) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

  const pattern = new RegExp(
    `^async function ${escaped}\\s*\\([^\\n]*\\)\\s*\\{[\\s\\S]*?^\\}`,
    'm'
  );

  if (!pattern.test(source)) {
    fail(`could not locate top-level async function ${name}`);
  }

  return source.replace(pattern, replacement);
}

function replaceTopLevelSyncFunction(source, name, replacement) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

  const pattern = new RegExp(
    `^function ${escaped}\\s*\\([^\\n]*\\)\\s*\\{[\\s\\S]*?^\\}`,
    'm'
  );

  if (!pattern.test(source)) {
    fail(`could not locate top-level function ${name}`);
  }

  return source.replace(pattern, replacement);
}

// Verify the pinned upstream version and original application identity.
const pkg = JSON.parse(read(packagePath));

if (pkg.version !== '3.8.53') {
  fail(`expected OmniRoute 3.8.53, found ${pkg.version}`);
}

if (pkg.build?.appId !== 'online.omniroute.desktop') {
  fail(`unexpected appId: ${pkg.build?.appId ?? '(missing)'}`);
}

// Apply ABUKHALED AI identity.
pkg.build.productName = 'ABUKHALED AI';
pkg.build.copyright = 'Copyright © 2026 ABUKHALED AI';

// Never publish this fork to upstream GitHub releases.
delete pkg.build.publish;

write(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);

// Read source and normalize Windows/Linux line endings.
let main = read(mainPath).replace(/\r\n?/g, '\n');

// Desktop identity.
main = replaceLiteral(
  main,
  'title: "OmniRoute",',
  'title: "ABUKHALED AI",',
  'window title'
);

main = replaceLiteral(
  main,
  'label: "Open OmniRoute",',
  'label: "Open ABUKHALED AI",',
  'tray menu label'
);

main = replaceLiteral(
  main,
  'tray.setToolTip("OmniRoute");',
  'tray.setToolTip("ABUKHALED AI");',
  'tray tooltip'
);

main = replaceLiteral(
  main,
  'title: "OmniRoute Update Ready",',
  'title: "ABUKHALED AI Update Ready",',
  'update notification title'
);

// Linux autostart identity.
main = replaceLiteral(
  main,
  '"Name=OmniRoute",',
  '"Name=ABUKHALED AI",',
  'Linux autostart name'
);

main = replaceLiteral(
  main,
  '"Comment=OmniRoute Desktop Client",',
  '"Comment=ABUKHALED AI Desktop Client",',
  'Linux autostart comment'
);

// Disable upstream update checks.
// Updates must eventually come from an ABUKHALED-owned release channel.
main = replaceTopLevelFunction(
  main,
  'checkForUpdates',
  `async function checkForUpdates(silent = false) {
  console.info("[ABUKHALED AI] Upstream auto-updates are disabled; use the ABUKHALED AI release channel.");

  if (!silent) {
    sendToRenderer("update-status", {
      status: "error",
      message: "Automatic updates are managed by ABUKHALED AI and are not available in this build.",
    });
  }

  return;
}`
);

// Disable upstream update downloads.
main = replaceTopLevelFunction(
  main,
  'downloadUpdate',
  `async function downloadUpdate() {
  console.info("[ABUKHALED AI] Upstream update downloads are disabled.");
  return;
}`
);

// Disable upstream update installation.
main = replaceTopLevelSyncFunction(
  main,
  'installUpdate',
  `function installUpdate() {
  console.info("[ABUKHALED AI] Upstream update installation is disabled.");
  return;
}`
);

write(mainPath, main);

// Copy ABUKHALED icons into Electron build resources.
for (const filename of ['icon.ico', 'icon.png', 'tray-icon.png']) {
  const source = path.join(assetsSourceDir, filename);

  if (!fs.existsSync(source)) {
    fail(`branding asset is missing: ${source}`);
  }

  fs.mkdirSync(assetsTargetDir, { recursive: true });
  fs.copyFileSync(source, path.join(assetsTargetDir, filename));
}

// Verify the final result.
const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);

if (finalPkg.version !== '3.8.53') {
  fail('version changed unexpectedly');
}

if (finalPkg.build?.appId !== 'online.omniroute.desktop') {
  fail('appId changed unexpectedly');
}

if (finalPkg.build?.productName !== 'ABUKHALED AI') {
  fail('productName branding was not applied');
}

if (Object.hasOwn(finalPkg.build || {}, 'publish')) {
  fail('upstream publish configuration remains enabled');
}

if (!finalMain.includes('Upstream auto-updates are disabled')) {
  fail('upstream update check was not disabled');
}

if (!finalMain.includes('Upstream update downloads are disabled')) {
  fail('upstream update download was not disabled');
}

if (!finalMain.includes('Upstream update installation is disabled')) {
  fail('upstream update installation was not disabled');
}

for (const filename of ['icon.ico', 'icon.png', 'tray-icon.png']) {
  if (!fs.existsSync(path.join(assetsTargetDir, filename))) {
    fail(`asset copy verification failed: ${filename}`);
  }
}

console.log('[ABUKHALED branding] Branding applied and postconditions passed.');
console.log('[ABUKHALED branding] Upstream version: 3.8.53');
console.log('[ABUKHALED branding] appId preserved: online.omniroute.desktop');
