
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

// Locate a complete top-level function using balanced braces.
function replaceFunction(source, name, replacement, isAsync = false) {
  const prefix = isAsync ? 'async function' : 'function';
  const declaration = new RegExp(
    `^${prefix}\\s+${name}\\s*\\([^\\n]*\\)\\s*\\{`,
    'm'
  );

  const match = declaration.exec(source);

  if (!match) {
    fail(`Could not locate ${prefix} ${name}`);
  }

  const start = match.index;
  const openBrace = source.indexOf('{', start);

  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  let templateExpressionDepth = 0;

  for (let i = openBrace; i < source.length; i++) {
    const char = source[i];
    const next = source[i + 1];

    if (lineComment) {
      if (char === '\n') lineComment = false;
      continue;
    }

    if (blockComment) {
      if (char === '*' && next === '/') {
        blockComment = false;
        i++;
      }
      continue;
    }

    if (quote) {
      if (escaped) {
        escaped = false;
        continue;
      }

      if (char === '\\') {
        escaped = true;
        continue;
      }

      if (quote === '`' && char === '$' && next === '{') {
        templateExpressionDepth++;
        depth++;
        i++;
        quote = null;
        continue;
      }

      if (char === quote) {
        quote = null;
      }
      continue;
    }

    if (char === '/' && next === '/') {
      lineComment = true;
      i++;
      continue;
    }

    if (char === '/' && next === '*') {
      blockComment = true;
      i++;
      continue;
    }

    if (char === '"' || char === "'" || char === '`') {
      quote = char;
      continue;
    }

    if (char === '{') {
      depth++;
      continue;
    }

    if (char === '}') {
      depth--;

      if (templateExpressionDepth > 0 && depth === 0) {
        templateExpressionDepth--;
        quote = '`';
        continue;
      }

      if (depth === 0) {
        const end = i + 1;
        return source.slice(0, start) + replacement + source.slice(end);
      }
    }
  }

  fail(`Could not find the closing brace of ${name}`);
}

// Verify upstream identity.
const pkg = JSON.parse(read(packagePath));

if (pkg.version !== '3.8.53') {
  fail(`Expected OmniRoute 3.8.53, found ${pkg.version}`);
}

if (pkg.build?.appId !== 'online.omniroute.desktop') {
  fail(`Unexpected appId: ${pkg.build?.appId ?? '(missing)'}`);
}

// Apply ABUKHALED AI identity.
pkg.build.productName = 'ABUKHALED AI';
pkg.build.copyright = 'Copyright © 2026 ABUKHALED AI';

delete pkg.build.publish;

fs.writeFileSync(
  packagePath,
  `${JSON.stringify(pkg, null, 2)}\n`,
  'utf8'
);

// Update Electron desktop identity.
let main = read(mainPath).replace(/\r\n?/g, '\n');

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

// Disable upstream automatic update checks.
main = replaceFunction(
  main,
  'checkForUpdates',
  `async function checkForUpdates(silent = false) {
  console.info("[ABUKHALED AI] Upstream auto-updates are disabled.");

  if (!silent) {
    sendToRenderer("update-status", {
      status: "error",
      message: "Updates are managed by ABUKHALED AI.",
    });
  }

  return;
}`,
  true
);

// Disable upstream update downloads.
main = replaceFunction(
  main,
  'downloadUpdate',
  `async function downloadUpdate() {
  console.info("[ABUKHALED AI] Upstream update downloads are disabled.");
  return;
}`,
  true
);

// Disable upstream update installation.
main = replaceFunction(
  main,
  'installUpdate',
  `function installUpdate() {
  console.info("[ABUKHALED AI] Upstream update installation is disabled.");
  return;
}`,
  false
);

fs.writeFileSync(mainPath, main, 'utf8');

// Copy ABUKHALED AI icon assets.
for (const filename of ['icon.ico', 'icon.png', 'tray-icon.png']) {
  const source = path.join(assetsSourceDir, filename);

  if (!fs.existsSync(source)) {
    fail(`Branding asset is missing: ${source}`);
  }

  fs.mkdirSync(assetsTargetDir, { recursive: true });
  fs.copyFileSync(source, path.join(assetsTargetDir, filename));
}

// Final verification.
const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);

if (finalPkg.version !== '3.8.53') {
  fail('Upstream version changed unexpectedly');
}

if (finalPkg.build?.appId !== 'online.omniroute.desktop') {
  fail('Original appId was not preserved');
}

if (finalPkg.build?.productName !== 'ABUKHALED AI') {
  fail('Product branding verification failed');
}

if (Object.hasOwn(finalPkg.build || {}, 'publish')) {
  fail('Upstream publish configuration remains enabled');
}

for (const marker of [
  'Upstream auto-updates are disabled',
  'Upstream update downloads are disabled',
  'Upstream update installation is disabled',
]) {
  if (!finalMain.includes(marker)) {
    fail(`Verification failed: ${marker}`);
  }
}

for (const filename of ['icon.ico', 'icon.png', 'tray-icon.png']) {
  if (!fs.existsSync(path.join(assetsTargetDir, filename))) {
    fail(`Asset verification failed: ${filename}`);
  }
}

console.log('[ABUKHALED branding] Branding applied successfully.');
console.log('[ABUKHALED branding] Version: 3.8.53');
console.log('[ABUKHALED branding] Original appId preserved.');
console.log('[ABUKHALED branding] Icon assets copied and verified.');
