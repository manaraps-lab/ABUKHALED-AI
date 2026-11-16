#!/usr/bin/env node
/**
 * ABUKHALED AI desktop startup readiness patch for pinned OmniRoute 3.8.53.
 * Changes only the Electron server-readiness probe from the lightweight
 * liveness endpoint to OmniRoute's lifecycle readiness endpoint.
 * The upstream engine, API routes, app identity, and user data are untouched.
 */
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(process.argv[2] || 'upstream');
const packagePath = path.join(root, 'electron', 'package.json');
const helperPath = path.join(root, 'electron', 'lib', 'serverReadiness.js');
const BRAND = 'ABUKHALED AI';

function fail(message) {
  console.error(`[${BRAND} readiness patch] ERROR: ${message}`);
  process.exit(1);
}

function read(filePath) {
  if (!fs.existsSync(filePath)) fail(`Required source file not found: ${filePath}`);
  return fs.readFileSync(filePath, 'utf8');
}

const pkg = JSON.parse(read(packagePath));
if (pkg.version !== '3.8.53') {
  fail(`Expected Electron package version 3.8.53; found ${pkg.version}`);
}
if (pkg.build?.appId !== 'online.omniroute.desktop') {
  fail(`Unexpected appId; refusing to modify source: ${pkg.build?.appId ?? '(missing)'}`);
}

const source = read(helperPath);
const oldProbe = 'return `${baseUrl.replace(/\\/+$/, "")}/api/health/ping`;';
const newProbe = 'return `${baseUrl.replace(/\\/+$/, "")}/healthz`;';
const oldCount = source.split(oldProbe).length - 1;
const newCount = source.split(newProbe).length - 1;

if (oldCount === 0 && newCount === 1) {
  console.log(`[${BRAND} readiness patch] Already applied; no changes needed.`);
  process.exit(0);
}
if (oldCount !== 1 || newCount !== 0) {
  fail(`Expected exactly one original readiness probe and no patched probe; found original=${oldCount}, patched=${newCount}`);
}

const updated = source.replace(oldProbe, newProbe);
if (updated === source || updated.includes('/api/health/ping')) {
  fail('Readiness endpoint replacement verification failed.');
}
fs.writeFileSync(helperPath, updated, 'utf8');

const verified = read(helperPath);
if (!verified.includes(newProbe) || verified.includes('/api/health/ping')) {
  fail('Post-write verification failed.');
}

console.log(`[${BRAND} readiness patch] Updated Electron startup probe to /healthz.`);
console.log(`[${BRAND} readiness patch] Upstream version: ${pkg.version}`);
console.log(`[${BRAND} readiness patch] Original appId preserved: ${pkg.build.appId}`);
