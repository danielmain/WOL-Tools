#!/usr/bin/env node

/**
 * Helper script to fetch Mozilla, Chrome, and Edge store credentials from Bitwarden CLI (bw)
 * and generate/update the local `.env` file.
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const rootDir = path.resolve(__dirname, '..');
const envPath = path.join(rootDir, '.env');

try {
  execSync('command -v bw', { stdio: 'ignore' });
} catch {
  console.error('\n❌ Bitwarden CLI (bw) not found in PATH.');
  console.error('Please install bw or ensure it is available in your PATH.\n');
  process.exit(1);
}

let bwStatus = {};
try {
  const statusOutput = execSync('bw status', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  bwStatus = JSON.parse(statusOutput);
} catch {
  // ignore
}

if (!process.env.BW_SESSION || bwStatus.status === 'locked') {
  console.log('\n🔒 Bitwarden vault is locked or BW_SESSION is not set in this terminal.');
  console.log('To unlock your vault and set your session, run:');
  console.log('\n  export BW_SESSION=$(bw unlock --raw)\n');
  console.log('After running that command, run:');
  console.log('  npm run pull:env\n');
  process.exit(1);
}

console.log('🔓 Bitwarden session active. Searching vault for store credentials...\n');

function runBw(cmd) {
  try {
    const out = execSync(`bw ${cmd}`, {
      encoding: 'utf8',
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe']
    });
    return JSON.parse(out);
  } catch {
    return null;
  }
}

function getField(item, names) {
  if (!item || !item.fields) return null;
  for (const name of names) {
    const field = item.fields.find((f) => f.name.toLowerCase() === name.toLowerCase());
    if (field && field.value) return field.value;
  }
  return null;
}

const existingEnv = {};
if (fs.existsSync(envPath)) {
  const lines = fs.readFileSync(envPath, 'utf8').split('\n');
  for (const line of lines) {
    const match = line.match(/^\s*([\w.-]+)\s*=\s*(.*)?\s*$/);
    if (match) {
      const key = match[1];
      let val = (match[2] || '').trim();
      if ((val.startsWith('"') && val.endsWith('"')) || (val.startsWith("'") && val.endsWith("'"))) {
        val = val.slice(1, -1);
      }
      existingEnv[key] = val;
    }
  }
}

const discovered = {};

// 1. Fetch Firefox item
let firefoxItem = runBw('get item "Firefox"');
if (!firefoxItem) {
  const results = runBw('list items --search "Firefox"') || [];
  firefoxItem = results.find((i) => (i.name || '').toLowerCase() === 'firefox') || results[0];
}

if (firefoxItem) {
  const key = getField(firefoxItem, ['WEB_EXT_API_KEY', 'api_key', 'jwt_issuer', 'issuer']) || firefoxItem.login?.username;
  const secret = getField(firefoxItem, ['WEB_EXT_API_SECRET', 'api_secret', 'jwt_secret', 'secret']) || firefoxItem.login?.password;
  if (key) discovered.WEB_EXT_API_KEY = key;
  if (secret) discovered.WEB_EXT_API_SECRET = secret;
  console.log(`  ✓ Found Mozilla credentials in item: "${firefoxItem.name}"`);
} else {
  console.log('  ⚠️ Item "Firefox" not found in Bitwarden.');
}

// 2. Fetch Google / Chrome item
let googleItem = runBw('get item "Google - Daniel"');
if (!googleItem) {
  const results = runBw('list items --search "Google - Daniel"') || [];
  googleItem = results.find((i) => (i.name || '').toLowerCase().includes('daniel')) || results[0];
}

if (googleItem) {
  const extId = getField(googleItem, ['CHROME_EXTENSION_ID', 'extension_id', 'item_id', 'app_id']);
  const clientId = getField(googleItem, ['CHROME_CLIENT_ID', 'client_id', 'oauth_client_id']) || googleItem.login?.username;
  const clientSecret = getField(googleItem, ['CHROME_CLIENT_SECRET', 'client_secret', 'oauth_client_secret']) || googleItem.login?.password;
  const refreshToken = getField(googleItem, ['CHROME_REFRESH_TOKEN', 'refresh_token', 'oauth_refresh_token']);

  if (extId) discovered.CHROME_EXTENSION_ID = extId;
  if (clientId) discovered.CHROME_CLIENT_ID = clientId;
  if (clientSecret) discovered.CHROME_CLIENT_SECRET = clientSecret;
  if (refreshToken) discovered.CHROME_REFRESH_TOKEN = refreshToken;

  console.log(`  ✓ Found Chrome credentials in item: "${googleItem.name}"`);
} else {
  console.log('  ⚠️ Item "Google - Daniel" not found in Bitwarden.');
}

// 3. Fetch Microsoft Edge item
let edgeItem = runBw('get item "Microsoft"') || runBw('get item "Microsoft Edge"') || runBw('get item "Edge"');
if (!edgeItem) {
  const results = [
    ...(runBw('list items --search "Microsoft"') || []),
    ...(runBw('list items --search "Edge"') || [])
  ];
  edgeItem = results.find((i) => getField(i, ['EDGE_PRODUCT_ID', 'EDGE_CLIENT_ID', 'EDGE_API_KEY']) !== null) ||
             results.find((i) => {
               const n = (i.name || '').toLowerCase();
               return n === 'microsoft' || n.includes('microsoft edge') || n === 'edge' || n.includes('partner center');
             });
}

if (edgeItem) {
  const productId = getField(edgeItem, ['EDGE_PRODUCT_ID', 'product_id', 'productid', 'item_id']);
  const clientId = getField(edgeItem, ['EDGE_CLIENT_ID', 'client_id', 'clientid']) || edgeItem.login?.username;
  const apiKey = getField(edgeItem, ['EDGE_API_KEY', 'api_key', 'apikey']);
  const clientSecret = getField(edgeItem, ['EDGE_CLIENT_SECRET', 'client_secret', 'clientsecret']) || (!apiKey ? edgeItem.login?.password : null);
  const accessTokenUrl = getField(edgeItem, ['EDGE_ACCESS_TOKEN_URL', 'access_token_url', 'token_url']);
  const tenantId = getField(edgeItem, ['EDGE_TENANT_ID', 'tenant_id', 'tenantid']);

  if (productId) discovered.EDGE_PRODUCT_ID = productId;
  if (clientId) discovered.EDGE_CLIENT_ID = clientId;
  if (apiKey) discovered.EDGE_API_KEY = apiKey;
  if (clientSecret) discovered.EDGE_CLIENT_SECRET = clientSecret;
  if (accessTokenUrl) discovered.EDGE_ACCESS_TOKEN_URL = accessTokenUrl;
  if (tenantId) discovered.EDGE_TENANT_ID = tenantId;

  console.log(`  ✓ Found Microsoft Edge credentials in item: "${edgeItem.name}"`);
} else {
  console.log('  ⚠️ Item "Microsoft" / "Microsoft Edge" / "Edge" not found in Bitwarden.');
}

const merged = {
  ...existingEnv,
  ...discovered
};

if (!merged.AMO_CHANNEL) {
  merged.AMO_CHANNEL = 'listed';
}
if (!merged.CHROME_PUBLISH_TARGET) {
  merged.CHROME_PUBLISH_TARGET = 'default';
}

let content = `# Store deployment secrets (Generated from Bitwarden via npm run pull:env)\n\n`;
content += `# Mozilla Firefox Add-ons (AMO)\n`;
content += `WEB_EXT_API_KEY="${merged.WEB_EXT_API_KEY || ''}"\n`;
content += `WEB_EXT_API_SECRET="${merged.WEB_EXT_API_SECRET || ''}"\n`;
content += `AMO_CHANNEL="${merged.AMO_CHANNEL || 'listed'}"\n\n`;

content += `# Google Chrome Web Store\n`;
content += `CHROME_EXTENSION_ID="${merged.CHROME_EXTENSION_ID || ''}"\n`;
content += `CHROME_CLIENT_ID="${merged.CHROME_CLIENT_ID || ''}"\n`;
content += `CHROME_CLIENT_SECRET="${merged.CHROME_CLIENT_SECRET || ''}"\n`;
content += `CHROME_REFRESH_TOKEN="${merged.CHROME_REFRESH_TOKEN || ''}"\n`;
content += `CHROME_PUBLISH_TARGET="${merged.CHROME_PUBLISH_TARGET || 'default'}"\n`;
content += `CHROME_AUTO_PUBLISH="${merged.CHROME_AUTO_PUBLISH || 'true'}"\n\n`;

content += `# Microsoft Edge Add-ons (Partner Center)\n`;
content += `EDGE_PRODUCT_ID="${merged.EDGE_PRODUCT_ID || ''}"\n`;
content += `EDGE_CLIENT_ID="${merged.EDGE_CLIENT_ID || ''}"\n`;
if (merged.EDGE_API_KEY || !merged.EDGE_CLIENT_SECRET) {
  content += `EDGE_API_KEY="${merged.EDGE_API_KEY || ''}"\n`;
}
if (merged.EDGE_CLIENT_SECRET) {
  content += `EDGE_CLIENT_SECRET="${merged.EDGE_CLIENT_SECRET || ''}"\n`;
  content += `EDGE_ACCESS_TOKEN_URL="${merged.EDGE_ACCESS_TOKEN_URL || ''}"\n`;
  content += `EDGE_TENANT_ID="${merged.EDGE_TENANT_ID || ''}"\n`;
}
content += `EDGE_AUTO_PUBLISH="${merged.EDGE_AUTO_PUBLISH || 'true'}"\n`;

fs.writeFileSync(envPath, content, 'utf8');

console.log(`\n💾 Saved credentials to ${envPath}`);

const missingMozilla = [];
if (!merged.WEB_EXT_API_KEY) missingMozilla.push('WEB_EXT_API_KEY');
if (!merged.WEB_EXT_API_SECRET) missingMozilla.push('WEB_EXT_API_SECRET');

const missingChrome = [];
if (!merged.CHROME_EXTENSION_ID) missingChrome.push('CHROME_EXTENSION_ID');
if (!merged.CHROME_CLIENT_ID) missingChrome.push('CHROME_CLIENT_ID');
if (!merged.CHROME_CLIENT_SECRET) missingChrome.push('CHROME_CLIENT_SECRET');
if (!merged.CHROME_REFRESH_TOKEN) missingChrome.push('CHROME_REFRESH_TOKEN');

const missingEdge = [];
if (!merged.EDGE_PRODUCT_ID) missingEdge.push('EDGE_PRODUCT_ID');
if (!merged.EDGE_CLIENT_ID) missingEdge.push('EDGE_CLIENT_ID');
if (!merged.EDGE_CLIENT_SECRET && !merged.EDGE_API_KEY) missingEdge.push('EDGE_CLIENT_SECRET');

const hasMozilla = missingMozilla.length === 0;
const hasChrome = missingChrome.length === 0;
const hasEdge = missingEdge.length === 0;

console.log('\nReady Status:');
console.log(`  Mozilla AMO credentials : ${hasMozilla ? '✅ Complete' : `⚠️ Missing: ${missingMozilla.join(', ')}`}`);
console.log(`  Chrome Web Store creds  : ${hasChrome ? '✅ Complete' : `⚠️ Missing: ${missingChrome.join(', ')}`}`);
console.log(`  Microsoft Edge Add-ons  : ${hasEdge ? '✅ Complete' : `⚠️ Missing: ${missingEdge.join(', ')}`}`);

console.log('\nNext steps:');
if (hasMozilla) console.log('  👉 Run: npm run publish:firefox');
if (hasChrome) console.log('  👉 Run: npm run publish:chrome');
if (hasEdge) console.log('  👉 Run: npm run publish:edge');
if (hasMozilla && hasChrome && hasEdge) console.log('  👉 Run: npm run publish');
