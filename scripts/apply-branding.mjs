import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(process.argv[2] || 'upstream');
const electronDir = path.join(root, 'electron');
const packagePath = path.join(electronDir, 'package.json');
const mainPath = path.join(electronDir, 'main.js');
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

function replaceLiteral(source, before, after, label) {
  const count = source.split(before).length - 1;
  if (count === 0 && source.includes(after)) return source;
  if (count !== 1) fail(`${label}: expected one source match (or an already branded value), found ${count}`);
  return source.replace(before, after);
}

// Locate a complete top-level function using balanced braces.
function replaceFunction(source, name, replacement, isAsync = false) {
  const prefix = isAsync ? 'async function' : 'function';
  const declaration = new RegExp(`^${prefix}\\s+${name}\\s*\\([^\\n]*\\)\\s*\\{`, 'm');
  const match = declaration.exec(source);
  if (!match) fail(`Could not locate ${prefix} ${name}`);

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
      if (char === '*' && next === '/') { blockComment = false; i++; }
      continue;
    }
    if (quote) {
      if (escaped) { escaped = false; continue; }
      if (char === '\\') { escaped = true; continue; }
      if (quote === '`' && char === '$' && next === '{') {
        templateExpressionDepth++;
        depth++;
        i++;
        quote = null;
        continue;
      }
      if (char === quote) quote = null;
      continue;
    }
    if (char === '/' && next === '/') { lineComment = true; i++; continue; }
    if (char === '/' && next === '*') { blockComment = true; i++; continue; }
    if (char === '"' || char === "'" || char === '`') { quote = char; continue; }
    if (char === '{') { depth++; continue; }
    if (char === '}') {
      depth--;
      if (templateExpressionDepth > 0 && depth === 0) {
        templateExpressionDepth--;
        quote = '`';
        continue;
      }
      if (depth === 0) return source.slice(0, start) + replacement + source.slice(i + 1);
    }
  }
  fail(`Could not find the closing brace of ${name}`);
}

function replaceBrandInValues(value) {
  if (typeof value === 'string') {
    return value
      .replace(/\bKeak\s+OmniRoute\b/g, BRAND)
      .replace(/OmniRoute/g, BRAND);
  }
  if (Array.isArray(value)) return value.map(replaceBrandInValues);
  if (value && typeof value === 'object') {
    for (const key of Object.keys(value)) value[key] = replaceBrandInValues(value[key]);
  }
  return value;
}

// Verify upstream identity before making any changes.
const pkg = JSON.parse(read(packagePath));
if (pkg.version !== '3.8.53') fail(`Expected OmniRoute 3.8.53, found ${pkg.version}`);
if (pkg.build?.appId !== 'online.omniroute.desktop') {
  fail(`Unexpected appId: ${pkg.build?.appId ?? '(missing)'}`);
}

// Electron package identity; keep the original appId to preserve existing user data.
pkg.build.productName = BRAND;
pkg.build.copyright = 'Copyright © 2026 ABUKHALED AI';
delete pkg.build.publish;
fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`, 'utf8');

// Electron window, tray and updater identity.
let main = read(mainPath).replace(/\r\n?/g, '\n');
main = replaceLiteral(main, 'title: "OmniRoute",', 'title: "ABUKHALED AI",', 'window title');
main = replaceLiteral(main, 'label: "Open OmniRoute",', 'label: "Open ABUKHALED AI",', 'tray menu label');
main = replaceLiteral(main, 'tray.setToolTip("OmniRoute");', 'tray.setToolTip("ABUKHALED AI");', 'tray tooltip');
main = replaceLiteral(main, 'title: "OmniRoute Update Ready",', 'title: "ABUKHALED AI Update Ready",', 'update notification title');
main = replaceLiteral(main, '"Name=OmniRoute",', '"Name=ABUKHALED AI",', 'Linux autostart name');
main = replaceLiteral(main, '"Comment=OmniRoute Desktop Client",', '"Comment=ABUKHALED AI Desktop Client",', 'Linux autostart comment');
main = replaceFunction(main, 'checkForUpdates', `async function checkForUpdates(silent = false) {
  console.info("[ABUKHALED AI] Upstream auto-updates are disabled.");
  if (!silent) {
    sendToRenderer("update-status", {
      status: "error",
      message: "Updates are managed by ABUKHALED AI.",
    });
  }
  return;
}`, true);
main = replaceFunction(main, 'downloadUpdate', `async function downloadUpdate() {
  console.info("[ABUKHALED AI] Upstream update downloads are disabled.");
  return;
}`, true);
main = replaceFunction(main, 'installUpdate', `function installUpdate() {
  console.info("[ABUKHALED AI] Upstream update installation is disabled.");
  return;
}`, false);
fs.writeFileSync(mainPath, main, 'utf8');

// Dashboard metadata and browser/PWA title.
let layout = read(layoutPath).replace(/\r\n?/g, '\n');
layout = replaceLiteral(
  layout,
  'const instanceName = settings.instanceName;',
  'const instanceName = "ABUKHALED AI";',
  'dashboard metadata application name'
);
layout = replaceLiteral(
  layout,
  '"OmniRoute is an AI gateway for multi-provider LLMs. One endpoint for all your AI providers.",',
  '"ABUKHALED AI is an AI gateway for multi-provider LLMs. One endpoint for your AI providers.",',
  'dashboard metadata description'
);
fs.writeFileSync(layoutPath, layout, 'utf8');

// Default application name used by the dashboard shell.
let appConfig = read(appConfigPath).replace(/\r\n?/g, '\n');
appConfig = replaceLiteral(appConfig, 'name: "OmniRoute",', 'name: "ABUKHALED AI",', 'application default name');
fs.writeFileSync(appConfigPath, appConfig, 'utf8');

// Force the product identity in the sidebar while retaining a user-supplied logo.
let sidebar = read(sidebarPath).replace(/\r\n?/g, '\n');
sidebar = replaceLiteral(
  sidebar,
  'const brandName = customAppName || APP_CONFIG.name;',
  'const brandName = APP_CONFIG.name;',
  'sidebar fixed brand name'
);
sidebar = replaceLiteral(
  sidebar,
  '<OmniRouteLockup\n                  name={customAppName}',
  '<OmniRouteLockup\n                  name={brandName}',
  'sidebar brand lockup name'
);
fs.writeFileSync(sidebarPath, sidebar, 'utf8');

// Remove the Keak cross-product switcher from the top bar; it is not part of ABUKHALED AI.
let header = read(headerPath).replace(/\r\n?/g, '\n');
header = replaceLiteral(
  header,
  'import { ProductSwitcher } from "@omniroute/ui-kit/keak-brand";\n',
  '',
  'Keak product switcher import'
);
header = replaceLiteral(
  header,
  'import { KEAK_ACTIVE_PRODUCT, KEAK_PRODUCT_LINKS } from "@/shared/constants/keakProducts";\n',
  '',
  'Keak product links import'
);
header = replaceLiteral(
  header,
  `        <ProductSwitcher\n          items={KEAK_PRODUCT_LINKS}\n          activeProduct={KEAK_ACTIVE_PRODUCT}\n          className="hidden md:block"\n          aria-label={t("products")}\n        />\n`,
  `        <span className="hidden md:inline-flex items-center text-sm font-semibold text-text-main">\n          ABUKHALED AI\n        </span>\n`,
  'Keak product switcher UI'
);
fs.writeFileSync(headerPath, header, 'utf8');

// Replace the OmniRoute/Keak product glyph with the ABUKHALED icon while preserving exports.
const logoSource = `"use client";\n\nimport { cn } from "@/shared/utils/cn";\n\ntype OmniRouteLogoProps = { size?: number; className?: string };\n\nexport default function OmniRouteLogo({ size = 20, className = "" }: OmniRouteLogoProps) {\n  return (\n    <img\n      src="/abukhaled-icon.png"\n      alt=""\n      width={size}\n      height={size}\n      aria-hidden="true"\n      className={cn("object-contain", className)}\n    />\n  );\n}\n\ninterface OmniRouteLockupProps {\n  name?: string | null;\n  logoSrc?: string | null;\n  className?: string;\n}\n\nexport function OmniRouteLockup({ name, logoSrc, className }: OmniRouteLockupProps) {\n  return (\n    <span\n      role="img"\n      aria-label={name || "ABUKHALED AI"}\n      className={cn("inline-flex items-center gap-2", className)}\n    >\n      <img\n        src={logoSrc || "/abukhaled-icon.png"}\n        alt=""\n        className="size-6 object-contain"\n      />\n      <span aria-hidden="true" className="truncate font-semibold">\n        {name || "ABUKHALED AI"}\n      </span>\n    </span>\n  );\n}\n`;
const previousLogo = read(logoPath);
if (!previousLogo.includes('ProductIcon') && !previousLogo.includes('/abukhaled-icon.png')) {
  fail('Unexpected logo component source; refusing to replace it.');
}
fs.writeFileSync(logoPath, logoSource, 'utf8');

// Replace brand mentions in user-facing translations, preserving translation keys and structure.
if (!fs.existsSync(messagesDir)) fail(`Translation directory not found: ${messagesDir}`);
const messageFiles = fs.readdirSync(messagesDir).filter((name) => name.endsWith('.json'));
if (messageFiles.length === 0) fail('No translation JSON files were found.');
for (const filename of messageFiles) {
  const file = path.join(messagesDir, filename);
  const messages = JSON.parse(read(file));
  const brandedMessages = replaceBrandInValues(messages);
  fs.writeFileSync(file, `${JSON.stringify(brandedMessages, null, 2)}\n`, 'utf8');
}

// Copy desktop icon assets and publish the same icon for the dashboard logo.
fs.mkdirSync(assetsTargetDir, { recursive: true });
fs.mkdirSync(publicDir, { recursive: true });
for (const filename of ['icon.ico', 'icon.png', 'tray-icon.png']) {
  const source = path.join(assetsSourceDir, filename);
  if (!fs.existsSync(source)) fail(`Branding asset is missing: ${source}`);
  fs.copyFileSync(source, path.join(assetsTargetDir, filename));
}
fs.copyFileSync(path.join(assetsSourceDir, 'icon.png'), path.join(publicDir, 'abukhaled-icon.png'));

// Final verification.
const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);
const finalLayout = read(layoutPath);
const finalAppConfig = read(appConfigPath);
const finalSidebar = read(sidebarPath);
const finalHeader = read(headerPath);
const finalLogo = read(logoPath);

if (finalPkg.version !== '3.8.53') fail('Upstream version changed unexpectedly');
if (finalPkg.build?.appId !== 'online.omniroute.desktop') fail('Original appId was not preserved');
if (finalPkg.build?.productName !== BRAND) fail('Product branding verification failed');
if (Object.hasOwn(finalPkg.build || {}, 'publish')) fail('Upstream publish configuration remains enabled');
for (const marker of [
  'Upstream auto-updates are disabled',
  'Upstream update downloads are disabled',
  'Upstream update installation is disabled',
]) {
  if (!finalMain.includes(marker)) fail(`Electron verification failed: ${marker}`);
}
for (const marker of [
  'const instanceName = "ABUKHALED AI";',
  'ABUKHALED AI is an AI gateway for multi-provider LLMs.',
]) {
  if (!finalLayout.includes(marker)) fail(`Dashboard metadata verification failed: ${marker}`);
}
if (!finalAppConfig.includes('name: "ABUKHALED AI",')) fail('Application default name verification failed');
if (!finalSidebar.includes('const brandName = APP_CONFIG.name;')) fail('Sidebar brand name verification failed');
if (!finalSidebar.includes('name={brandName}')) fail('Sidebar lockup verification failed');
if (finalHeader.includes('ProductSwitcher') || finalHeader.includes('KEAK_PRODUCT_LINKS')) {
  fail('Keak product switcher is still present in the dashboard header');
}
if (!finalHeader.includes('ABUKHALED AI')) fail('Dashboard header brand verification failed');
if (!finalLogo.includes('/abukhaled-icon.png')) fail('ABUKHALED logo asset is not wired into the logo component');
if (!fs.existsSync(path.join(publicDir, 'abukhaled-icon.png'))) fail('Dashboard logo image was not copied');

const arabicMessages = JSON.parse(read(path.join(messagesDir, 'ar.json')));
const hasOldBrand = (value) => {
  if (typeof value === 'string') return value.includes('OmniRoute') || value.includes('Keak OmniRoute');
  if (Array.isArray(value)) return value.some(hasOldBrand);
  if (value && typeof value === 'object') return Object.values(value).some(hasOldBrand);
  return false;
};
if (hasOldBrand(arabicMessages)) fail('Arabic translations still contain the old product brand');

console.log('[ABUKHALED branding] Full dashboard branding applied successfully.');
console.log('[ABUKHALED branding] Version: 3.8.53');
console.log('[ABUKHALED branding] Original appId preserved.');
console.log('[ABUKHALED branding] Electron window and tray identity verified.');
console.log('[ABUKHALED branding] Sidebar and header identity verified.');
console.log(`[ABUKHALED branding] Updated ${messageFiles.length} translation files.`);
console.log('[ABUKHALED branding] ABUKHALED icon copied and verified.');
