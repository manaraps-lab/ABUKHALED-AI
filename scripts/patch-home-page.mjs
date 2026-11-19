
/**
 * ABUKHALED AI home-page cleanup for OmniRoute 3.8.53.
 * Removes sponsor/news banners while preserving essential dashboard components.
 */
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(process.argv[2] || 'upstream');
const file = path.join(root, 'src', 'app', '(dashboard)', 'home', 'page.tsx');
const marker = 'ABUKHALED_HOME_CLEANUP_V1';

function fail(message) {
  console.error(`[ABUKHALED home cleanup] ERROR: ${message}`);
  process.exit(1);
}

if (!fs.existsSync(file)) fail(`Required source file not found: ${file}`);

let source = fs.readFileSync(file, 'utf8').replace(/\r\n?/g, '\n');

if (source.includes(marker)) {
  console.log('[ABUKHALED home cleanup] Already applied; no changes needed.');
  process.exit(0);
}

const expectedImports = [
  'import KimiSponsorBanner from "../dashboard/KimiSponsorBanner";\n',
  'import CheaperInferenceSponsorBanner from "../dashboard/CheaperInferenceSponsorBanner";\n',
  'import VscodeCopilotBanner from "../dashboard/VscodeCopilotBanner";\n',
  'import NewsBanner from "../dashboard/NewsBanner";\n',
];

for (const line of expectedImports) {
  const count = source.split(line).length - 1;
  if (count !== 1) {
    fail(`Expected exactly one import to remove; found ${count}: ${line.trim()}`);
  }
  source = source.replace(line, '');
}

const oldRender = `      <KimiSponsorBanner />
      <CheaperInferenceSponsorBanner />
      <VscodeCopilotBanner />
      <NewsBanner />
`;

const count = source.split(oldRender).length - 1;
if (count !== 1) {
  fail(`Expected exactly one sponsor/news render block; found ${count}`);
}

source = source.replace(
  oldRender,
  `      {/* ${marker}: sponsor and news promotions intentionally omitted */}\n`
);

for (const required of [
  'import BootstrapBanner from "../dashboard/BootstrapBanner";',
  'import FirstRunReadinessCard from "../dashboard/FirstRunReadinessCard";',
  'import UnpricedUsageBanner from "../dashboard/UnpricedUsageBanner";',
  'import HomePageClient from "../dashboard/HomePageClient";',
  '{isBootstrapped && <BootstrapBanner />}',
  '<FirstRunReadinessCard setupComplete={Boolean(settings.setupComplete)} />',
  '<UnpricedUsageBanner />',
  '<HomePageClient machineId={machineId} />',
]) {
  if (!source.includes(required)) {
    fail(`Required home functionality was not preserved: ${required}`);
  }
}

fs.writeFileSync(file, source, 'utf8');

console.log(
  '[ABUKHALED home cleanup] Sponsor/news banners removed. ' +
  'Bootstrap, readiness, usage notice, and HomePageClient preserved.'
);
