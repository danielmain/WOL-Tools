#!/usr/bin/env node

/**
 * Build & Publish script for Microsoft Edge Add-ons (Partner Center).
 *
 * Requirements (in `.env` or environment):
 * - EDGE_PRODUCT_ID
 * - EDGE_CLIENT_ID
 * - EDGE_CLIENT_SECRET (or EDGE_API_KEY)
 * - EDGE_ACCESS_TOKEN_URL or EDGE_TENANT_ID (if using Client Secret OAuth)
 *
 * Optional:
 * - EDGE_AUTO_PUBLISH ("true" or "false", default: "true")
 * - EDGE_NOTES (Certification notes string)
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
      console.warn(`[publish-edge] Warning loading .env: ${err.message}`);
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

const productId = process.env.EDGE_PRODUCT_ID;
const clientId = process.env.EDGE_CLIENT_ID;
const apiKey = process.env.EDGE_API_KEY;
const clientSecret = process.env.EDGE_CLIENT_SECRET;
const tenantId = process.env.EDGE_TENANT_ID;
const tokenUrl = process.env.EDGE_ACCESS_TOKEN_URL || (tenantId ? `https://login.microsoftonline.com/${tenantId}/oauth2/v2.0/token` : null);

const args = process.argv.slice(2);
const isDraftOnly = args.includes('--draft') || process.env.EDGE_AUTO_PUBLISH === 'false';
const skipBuild = args.includes('--no-build');
const customNotes = process.env.EDGE_NOTES || '';

if (args.includes('--help') || args.includes('-h')) {
  console.log('Usage: npm run publish:edge [-- options]');
  console.log('\nOptions:');
  console.log('  --bump        Automatically bump patch version before publishing');
  console.log('  --draft       Upload package to Edge Add-ons as draft without submitting for review');
  console.log('  --no-build    Skip rebuilding the zip package prior to upload');
  console.log('  --help, -h    Display this help message');
  process.exit(0);
}

if (args.includes('--bump')) {
  console.log('📦 Bumping patch version before publishing to Microsoft Edge...\n');
  const { spawnSync } = require('child_process');
  spawnSync('node', [path.join(rootDir, 'scripts', 'bump-version.js'), 'patch'], {
    cwd: rootDir,
    stdio: 'inherit'
  });
}

if (!productId) {
  console.error('\n❌ Missing EDGE_PRODUCT_ID in .env or environment!\n');
  process.exit(1);
}

if (!apiKey && (!clientId || !clientSecret || !tokenUrl)) {
  console.error('\n❌ Incomplete Microsoft Edge credentials!');
  console.error('Please configure either EDGE_API_KEY OR (EDGE_CLIENT_ID, EDGE_CLIENT_SECRET, and EDGE_ACCESS_TOKEN_URL / EDGE_TENANT_ID) in .env.\n');
  process.exit(1);
}

const EDGE_API_BASE = 'https://api.addons.microsoftedge.com';

async function getAuthHeaders() {
  if (apiKey) {
    return { 'ApiKey': apiKey };
  }

  console.log('🔑 Requesting Microsoft OAuth2 token...');
  const body = new URLSearchParams({
    client_id: clientId,
    client_secret: clientSecret,
    scope: 'https://api.addons.microsoftedge.com/.default',
    grant_type: 'client_credentials'
  });

  const res = await fetch(tokenUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: body.toString()
  });

  const data = await res.json();
  if (!res.ok || !data.access_token) {
    throw new Error(`Failed to acquire Microsoft OAuth token: ${data.error_description || data.error || res.statusText}`);
  }

  console.log('  ✓ Microsoft OAuth token acquired.');
  return { 'Authorization': `Bearer ${data.access_token}` };
}

function resolveOperationUrl(endpoint, location) {
  if (location.startsWith('http://') || location.startsWith('https://')) {
    return location;
  }
  const base = endpoint.split('/products/')[0];
  return `${base}/${location.replace(/^\/+/, '')}`;
}

async function pollOperation(operationUrl, authHeaders, operationName) {
  console.log(`⏳ Tracking status of: ${operationName}...`);
  for (let i = 0; i < 30; i++) {
    await new Promise((r) => setTimeout(r, 5000));
    const res = await fetch(operationUrl, { headers: authHeaders });
    if (!res.ok) {
      console.warn(`  ⚠️ Poll warning: HTTP ${res.status}`);
      continue;
    }
    const data = await res.json();
    const status = (data.status || '').toLowerCase();
    console.log(`  ... current status: ${data.status || 'InProgress'}`);

    if (status === 'succeeded' || status === 'completed') {
      console.log(`  ✓ ${operationName} finished successfully!`);
      return data;
    }
    if (status === 'failed') {
      throw new Error(`${operationName} failed: ${JSON.stringify(data.errors || data.message || data)}`);
    }
  }
  throw new Error(`${operationName} timed out while waiting for completion.`);
}

async function uploadZip(authHeaders, zipPath) {
  const zipBuffer = fs.readFileSync(zipPath);
  const sizeMb = (zipBuffer.length / (1024 * 1024)).toFixed(2);
  const fileName = path.basename(zipPath);

  console.log(`📦 Uploading ${fileName} (${sizeMb} MB) to Edge Partner Center...`);

  const uploadEndpoint = `${EDGE_API_BASE}/v1/products/${productId}/submissions/draft/package`;
  const res = await fetch(uploadEndpoint, {
    method: 'POST',
    headers: {
      ...authHeaders,
      'Content-Type': 'application/zip'
    },
    body: zipBuffer
  });

  if (res.status !== 202 && !res.ok) {
    const errorText = await res.text();
    throw new Error(`Package upload failed (HTTP ${res.status}): ${errorText}`);
  }

  const locationHeader = res.headers.get('location') || res.headers.get('Location');
  console.log(`  ✓ Package upload accepted (HTTP ${res.status}).`);

  if (locationHeader) {
    const opUrl = resolveOperationUrl(uploadEndpoint, locationHeader);
    await pollOperation(opUrl, authHeaders, 'Package upload verification');
  }
}

async function publishDraft(authHeaders) {
  console.log('🚀 Submitting draft package for review on Microsoft Edge Add-ons...');
  const publishEndpoint = `${EDGE_API_BASE}/v1/products/${productId}/submissions`;
  const bodyPayload = customNotes ? { notes: customNotes } : {};

  const res = await fetch(publishEndpoint, {
    method: 'POST',
    headers: {
      ...authHeaders,
      'Content-Type': 'application/json'
    },
    body: JSON.stringify(bodyPayload)
  });

  if (res.status !== 202 && !res.ok) {
    const errorText = await res.text();
    throw new Error(`Publish submission failed (HTTP ${res.status}): ${errorText}`);
  }

  const locationHeader = res.headers.get('location') || res.headers.get('Location');
  console.log(`  ✓ Publish request accepted (HTTP ${res.status}).`);

  if (locationHeader) {
    const opUrl = resolveOperationUrl(publishEndpoint, locationHeader);
    await pollOperation(opUrl, authHeaders, 'Publish submission');
  }
}

async function main() {
  console.log('====================================================');
  console.log('🌊 Building & Publishing to Microsoft Edge Add-ons...');
  console.log(`   Product ID: ${productId}`);
  console.log(`   Mode: ${isDraftOnly ? 'Upload Draft Only' : 'Upload & Publish'}`);
  console.log('====================================================\n');

  const distDir = path.join(rootDir, 'dist');
  if (!skipBuild) {
    console.log('🔨 Building latest Chromium package (npm run build:chrome)...');
    execSync('npm run build:chrome', { cwd: rootDir, stdio: 'inherit' });
  }

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
  const authHeaders = await getAuthHeaders();

  await uploadZip(authHeaders, zipPath);

  if (isDraftOnly) {
    console.log('\n💡 Extension package uploaded as draft (skip publish requested).');
    console.log('   Go to Partner Center Dashboard to review and publish manually.\n');
  } else {
    await publishDraft(authHeaders);
    console.log('\n✅ Microsoft Edge Add-ons submission succeeded!\n');
  }
}

main().catch((err) => {
  console.error('\n❌ Microsoft Edge Add-ons submission failed:');
  console.error(err.message);
  process.exit(1);
});
