#!/usr/bin/env node

/**
 * Build & Publish script for Mozilla Add-ons, Chrome Web Store, and Microsoft Edge Add-ons.
 */

const path = require('path');
const { spawnSync } = require('child_process');

const rootDir = path.resolve(__dirname, '..');
const args = process.argv.slice(2);

const shouldBump = args.includes('--bump');
const passArgs = args.filter((a) => a !== '--bump');

if (shouldBump) {
  console.log('📦 Bumping patch version before publishing...\n');
  spawnSync('node', [path.join(rootDir, 'scripts', 'bump-version.js'), 'patch'], {
    cwd: rootDir,
    stdio: 'inherit'
  });
}

console.log('####################################################');
console.log('# Multi-Store Publisher: Firefox, Chrome & Edge    #');
console.log('####################################################\n');

let firefoxStatus = 0;
let chromeStatus = 0;
let edgeStatus = 0;

// 1. Publish to Firefox
console.log('▶ [1/3] Starting Firefox Add-ons submission...\n');
const firefoxRes = spawnSync('node', [path.join(rootDir, 'scripts', 'publish-firefox.js'), ...passArgs], {
  cwd: rootDir,
  stdio: 'inherit'
});
firefoxStatus = firefoxRes.status;

console.log('\n----------------------------------------------------\n');

// 2. Publish to Chrome
console.log('▶ [2/3] Starting Chrome Web Store submission...\n');
const chromeRes = spawnSync('node', [path.join(rootDir, 'scripts', 'publish-chrome.js'), ...passArgs], {
  cwd: rootDir,
  stdio: 'inherit'
});
chromeStatus = chromeRes.status;

console.log('\n----------------------------------------------------\n');

// 3. Publish to Edge
console.log('▶ [3/3] Starting Microsoft Edge Add-ons submission...\n');
const edgeRes = spawnSync('node', [path.join(rootDir, 'scripts', 'publish-edge.js'), ...passArgs], {
  cwd: rootDir,
  stdio: 'inherit'
});
edgeStatus = edgeRes.status;

console.log('\n====================================================');
console.log('📊 Deployment Summary:');
console.log(`   Mozilla Firefox AMO:   ${firefoxStatus === 0 ? '✅ SUCCESS' : '❌ FAILED'}`);
console.log(`   Google Chrome Store:   ${chromeStatus === 0 ? '✅ SUCCESS' : '❌ FAILED'}`);
console.log(`   Microsoft Edge Addons: ${edgeStatus === 0 ? '✅ SUCCESS' : '❌ FAILED'}`);
console.log('====================================================\n');

if (firefoxStatus !== 0 || chromeStatus !== 0 || edgeStatus !== 0) {
  process.exit(1);
}
