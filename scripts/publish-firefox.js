#!/usr/bin/env node

/**
 * Build & Publish script for Mozilla Firefox Add-ons (AMO).
 *
 * Requirements:
 * - WEB_EXT_API_KEY (JWT Issuer)
 * - WEB_EXT_API_SECRET (JWT Secret)
 *
 * Place them in a `.env` file in the project root, or set as environment variables.
 */

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const rootDir = path.resolve(__dirname, '..');
const envPath = path.join(rootDir, '.env');
const sourceDir = path.join(rootDir, 'wol-highlights');
const distDir = path.join(rootDir, 'dist');

// Load .env if present
if (fs.existsSync(envPath)) {
  if (typeof process.loadEnvFile === 'function') {
    try {
      process.loadEnvFile(envPath);
    } catch (err) {
      console.warn(`[publish-firefox] Warning loading .env: ${err.message}`);
    }
  } else {
    const lines = fs.readFileSync(envPath, 'utf8').split('\n');
    for (const line of lines) {
      const match = line.match(/^\s*([\w.-]+)\s*=\s*(.*)?\s*$/);
      if (match) {
        const key = match[1];
        let val = (match[2] || '').trim();
        if ((val.startsWith('"') && val.endsWith('"')) || (val.startsWith("'") && val.endsWith("'"))) {
          val = val.slice(1, -1);
        }
        if (!process.env[key]) {
          process.env[key] = val;
        }
      }
    }
  }
}

let userArgs = process.argv.slice(2);

if (userArgs.includes('--help') || userArgs.includes('-h')) {
  console.log('Usage: npm run publish:firefox [-- [web-ext sign options]]');
  console.log('\nOptions:');
  console.log('  --bump                        Automatically bump patch version before publishing');
  console.log('  --channel <listed|unlisted>   Default: "listed" (public store) or AMO_CHANNEL');
  console.log('  --approval-timeout <ms>       Wait for approval (set 0 to exit right after upload)');
  console.log('  All other flags are passed directly to `web-ext sign`.');
  process.exit(0);
}

if (userArgs.includes('--bump')) {
  console.log('📦 Bumping patch version before publishing to Mozilla Add-ons...\n');
  spawnSync('node', [path.join(rootDir, 'scripts', 'bump-version.js'), 'patch'], {
    cwd: rootDir,
    stdio: 'inherit'
  });
  userArgs = userArgs.filter((a) => a !== '--bump');
}

const apiKey = process.env.WEB_EXT_API_KEY;
const apiSecret = process.env.WEB_EXT_API_SECRET;

if (!apiKey || !apiSecret) {
  console.error('\n❌ Missing required Mozilla Add-ons (AMO) API credentials!\n');
  console.error('Please configure your credentials in a `.env` file in the project root:');
  console.error('  WEB_EXT_API_KEY="user:xxxxxxx:xxx"       # JWT Issuer');
  console.error('  WEB_EXT_API_SECRET="xxxxxxxxxxxxxxxx..."  # JWT Secret\n');
  console.error('👉 You can generate your Mozilla JWT credentials at:');
  console.error('   https://addons.mozilla.org/developers/addon/api/key/\n');
  process.exit(1);
}

const channel = process.env.AMO_CHANNEL || 'listed';
const approvalTimeout = process.env.AMO_APPROVAL_TIMEOUT;

const webExtArgs = [
  'web-ext',
  'sign',
  '--source-dir',
  sourceDir,
  '--artifacts-dir',
  distDir,
  '--api-key',
  apiKey,
  '--api-secret',
  apiSecret,
  '--ignore-files',
  'test',
  'test/**'
];

const hasChannelArg = userArgs.some((arg) => arg === '--channel' || arg.startsWith('--channel='));
if (!hasChannelArg) {
  webExtArgs.push('--channel', channel);
}

const hasApprovalTimeout = userArgs.some((arg) => arg === '--approval-timeout' || arg.startsWith('--approval-timeout='));
if (!hasApprovalTimeout && approvalTimeout !== undefined) {
  webExtArgs.push('--approval-timeout', approvalTimeout);
}

webExtArgs.push(...userArgs);

console.log('====================================================');
console.log('🦊 Building & Submitting to Mozilla Add-ons (AMO)...');
console.log(`   Channel: ${hasChannelArg ? '(from CLI)' : channel}`);
console.log('====================================================\n');

const result = spawnSync('npx', webExtArgs, {
  cwd: rootDir,
  stdio: 'inherit'
});

if (result.status !== 0) {
  console.error('\n❌ Mozilla Add-ons submission failed with exit code:', result.status);
  process.exit(result.status || 1);
}

console.log('\n✅ Mozilla Add-ons submission succeeded!\n');
