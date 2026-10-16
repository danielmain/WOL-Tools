#!/usr/bin/env node

/**
 * Build & Publish script for Google Chrome Web Store.
 *
 * Requirements (in `.env` or environment):
 * - CHROME_EXTENSION_ID
 * - CHROME_CLIENT_ID
 * - CHROME_CLIENT_SECRET
 * - CHROME_REFRESH_TOKEN
 *
 * Optional:
 * - CHROME_PUBLISH_TARGET ("default" or "trustedTesters")
 * - CHROME_AUTO_PUBLISH ("true" or "false", default: "true")
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const rootDir = path.resolve(__dirname, '..');
const envPath = path.join(rootDir, '.env');

// Load .env if present
if (fs.existsSync(envPath)) {
  if (typeof process.loadEnvFile === 'function') {
    try {
      process.loadEnvFile(envPath);
    } catch (err) {
      console.warn(`[publish-chrome] Warning loading .env: ${err.message}`);
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

const extensionId = process.env.CHROME_EXTENSION_ID;
const clientId = process.env.CHROME_CLIENT_ID;
const clientSecret = process.env.CHROME_CLIENT_SECRET;
const refreshToken = process.env.CHROME_REFRESH_TOKEN;

const args = process.argv.slice(2);
const isDraftOnly = args.includes('--draft') || process.env.CHROME_AUTO_PUBLISH === 'false';
const skipBuild = args.includes('--no-build');

if (args.includes('--help') || args.includes('-h')) {
  console.log('Usage: npm run publish:chrome [-- options]');
  console.log('\nOptions:');
  console.log('  --bump        Automatically bump patch version before publishing');
  console.log('  --draft       Upload package to Chrome Web Store as a draft without submitting for review');
  console.log('  --no-build    Skip rebuilding the zip package prior to upload');
  console.log('  --help, -h    Display this help message');
  process.exit(0);
}

if (args.includes('--bump')) {
  console.log('📦 Bumping patch version before publishing to Chrome Web Store...\n');
  const { spawnSync } = require('child_process');
  spawnSync('node', [path.join(rootDir, 'scripts', 'bump-version.js'), 'patch'], {
    cwd: rootDir,
    stdio: 'inherit'
  });
}

if (!extensionId || !clientId || !clientSecret || !refreshToken) {
  console.error('\n❌ Missing required Chrome Web Store credentials!\n');
  console.error('Please configure your credentials in a `.env` file in the project root:');
  console.error('  CHROME_EXTENSION_ID="abcdefghijklmnopqrstuvwxyzabcdef"');
  console.error('  CHROME_CLIENT_ID="123456789...apps.googleusercontent.com"');
  console.error('  CHROME_CLIENT_SECRET="GOCSPX-..."');
  console.error('  CHROME_REFRESH_TOKEN="1//0..."\n');
  console.error('To obtain these credentials:');
  console.error('  1. Chrome Web Store: Get extension ID from your Developer Dashboard.');
  console.error('  2. Google Cloud Console: Enable the "Chrome Web Store API" in your project.');
  console.error('  3. Create an OAuth 2.0 Client ID (type Desktop/Web) and get Client ID & Secret.');
  console.error('  4. Run `npm run auth:chrome` to generate the Refresh Token.\n');
  process.exit(1);
}

async function getAccessToken() {
  console.log('🔑 Requesting Google OAuth2 access token...');
  const body = new URLSearchParams({
    client_id: clientId,
    client_secret: clientSecret,
    refresh_token: refreshToken,
    grant_type: 'refresh_token'
  });

  const res = await fetch('https://oauth2.googleapis.com/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: body.toString()
  });

  const data = await res.json();
  if (!res.ok || !data.access_token) {
    throw new Error(`Failed to acquire OAuth access token: ${data.error_description || data.error || res.statusText}`);
  }

  console.log('  ✓ OAuth access token acquired.');
  return data.access_token;
}

async function uploadZip(accessToken, zipPath) {
  const zipBuffer = fs.readFileSync(zipPath);
  const sizeMb = (zipBuffer.length / (1024 * 1024)).toFixed(2);
  const fileName = path.basename(zipPath);

  console.log(`📦 Uploading ${fileName} (${sizeMb} MB) to Chrome Web Store...`);

  const uploadUrl = `https://www.googleapis.com/upload/chromewebstore/v1.1/items/${extensionId}`;
  const res = await fetch(uploadUrl, {
    method: 'PUT',
    headers: {
      Authorization: `Bearer ${accessToken}`,
      'x-goog-api-version': '2'
    },
    body: zipBuffer
  });

  const data = await res.json();
  if (!res.ok || data.uploadState === 'FAILURE') {
    const errorDetails = data.itemError ? JSON.stringify(data.itemError, null, 2) : res.statusText;
    throw new Error(`Upload failed (status: ${data.uploadState || res.status}): ${errorDetails}`);
  }

  console.log(`  ✓ Package uploaded successfully (State: ${data.uploadState}).`);
  return data;
}

async function publishItem(accessToken) {
  const target = process.env.CHROME_PUBLISH_TARGET || 'default';
  console.log(`🚀 Submitting package for review / publication (Target: ${target})...`);

  const publishUrl = `https://www.googleapis.com/chromewebstore/v1.1/items/${extensionId}/publish`;
  const res = await fetch(publishUrl, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${accessToken}`,
      'x-goog-api-version': '2',
      'Content-Type': 'application/json'
    },
    body: JSON.stringify({ target })
  });

  const data = await res.json();
  if (!res.ok) {
    const details = data.error ? JSON.stringify(data.error, null, 2) : res.statusText;
    throw new Error(`Publish request failed: ${details}`);
  }

  const status = (data.status || []).join(', ');
  const detail = (data.statusDetail || []).join(', ');
  console.log(`  ✓ Publish submitted. Status: ${status || 'OK'}${detail ? ` (${detail})` : ''}`);
  return data;
}

async function main() {
  console.log('====================================================');
  console.log('🌐 Building & Publishing to Chrome Web Store...');
  console.log(`   Extension ID: ${extensionId}`);
  console.log(`   Mode: ${isDraftOnly ? 'Upload Draft Only' : 'Upload & Publish'}`);
  console.log('====================================================\n');

  // Step 1: Build package
  const distDir = path.join(rootDir, 'dist');
  if (!skipBuild) {
    console.log('🔨 Building latest Chrome package (npm run build:chrome)...');
    execSync('npm run build:chrome', { cwd: rootDir, stdio: 'inherit' });
  }

  // Find zip in dist/
  if (!fs.existsSync(distDir)) {
    throw new Error(`dist directory not found: ${distDir}`);
  }
  const zipFiles = fs.readdirSync(distDir).filter((f) => f.endsWith('.zip'));
  if (zipFiles.length === 0) {
    throw new Error('No .zip archive found in dist/. Please run `npm run build:chrome` first.');
  }

  const latestZip = zipFiles
    .map((name) => ({
      name,
      mtime: fs.statSync(path.join(distDir, name)).mtimeMs
    }))
    .sort((a, b) => b.mtime - a.mtime)[0].name;

  const zipPath = path.join(distDir, latestZip);

  // Step 2: Acquire Access Token
  const accessToken = await getAccessToken();

  // Step 3: Upload Zip
  await uploadZip(accessToken, zipPath);

  // Step 4: Publish (if not draft only)
  if (isDraftOnly) {
    console.log('\n💡 Extension package uploaded as draft (skip publish requested).');
    console.log('   Go to the Chrome Web Store Developer Dashboard to review and publish manually.\n');
  } else {
    await publishItem(accessToken);
    console.log('\n✅ Chrome Web Store submission succeeded!\n');
  }
}

main().catch((err) => {
  console.error('\n❌ Chrome Web Store submission failed:');
  console.error(err.message);
  process.exit(1);
});
