#!/usr/bin/env node

/**
 * Interactive OAuth 2.0 helper for Desktop Client IDs.
 * Starts a temporary local server, opens the browser for authorization,
 * captures the code, exchanges it for a refresh token, and updates .env.
 */

const http = require('http');
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const rootDir = path.resolve(__dirname, '..');
const envPath = path.join(rootDir, '.env');

// Load .env
if (fs.existsSync(envPath) && typeof process.loadEnvFile === 'function') {
  try {
    process.loadEnvFile(envPath);
  } catch (e) {
    // ignore
  }
}

const clientId = process.env.CHROME_CLIENT_ID;
const clientSecret = process.env.CHROME_CLIENT_SECRET;

if (!clientId || !clientSecret) {
  console.error('\n❌ Missing CHROME_CLIENT_ID or CHROME_CLIENT_SECRET in .env!\n');
  process.exit(1);
}

const PORT = 8989;
const REDIRECT_URI = `http://127.0.0.1:${PORT}`;
const SCOPE = 'https://www.googleapis.com/auth/chromewebstore';

const authUrl =
  `https://accounts.google.com/o/oauth2/v2/auth?` +
  new URLSearchParams({
    client_id: clientId,
    response_type: 'code',
    redirect_uri: REDIRECT_URI,
    scope: SCOPE,
    access_type: 'offline',
    prompt: 'consent'
  }).toString();

console.log('====================================================');
console.log('🔑 Chrome Web Store OAuth 2.0 Authorization');
console.log('====================================================\n');
console.log('1. Starting temporary local callback listener on port', PORT);
console.log('2. Opening your browser to approve permissions...\n');
console.log(`URL: ${authUrl}\n`);

const server = http.createServer(async (req, res) => {
  const reqUrl = new URL(req.url, REDIRECT_URI);
  const code = reqUrl.searchParams.get('code');
  const error = reqUrl.searchParams.get('error');

  if (error) {
    res.writeHead(400, { 'Content-Type': 'text/html' });
    res.end(`<h2>Authorization Error: ${error}</h2><p>You may close this window.</p>`);
    console.error(`\n❌ Authorization rejected by Google: ${error}`);
    server.close();
    process.exit(1);
  }

  if (code) {
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end(`<h2>Authorization Successful!</h2><p>Exchanging code for tokens... You can close this tab.</p>`);
    server.close();

    console.log('✓ Authorization code received. Exchanging for Refresh Token...\n');

    try {
      const tokenRes = await fetch('https://oauth2.googleapis.com/token', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({
          client_id: clientId,
          client_secret: clientSecret,
          code,
          grant_type: 'authorization_code',
          redirect_uri: REDIRECT_URI
        }).toString()
      });

      const tokenData = await tokenRes.json();

      if (!tokenRes.ok || !tokenData.refresh_token) {
        console.error('❌ Failed to retrieve refresh token:');
        console.error(tokenData);
        process.exit(1);
      }

      const refreshToken = tokenData.refresh_token;
      console.log('🎉 Successfully acquired Refresh Token!\n');

      let envContent = fs.existsSync(envPath) ? fs.readFileSync(envPath, 'utf8') : '';
      if (envContent.includes('CHROME_REFRESH_TOKEN=')) {
        envContent = envContent.replace(
          /CHROME_REFRESH_TOKEN=.*(\r?\n|$)/,
          `CHROME_REFRESH_TOKEN="${refreshToken}"$1`
        );
      } else {
        envContent += `\nCHROME_REFRESH_TOKEN="${refreshToken}"\n`;
      }

      fs.writeFileSync(envPath, envContent, 'utf8');
      console.log(`✓ Updated ${envPath} with CHROME_REFRESH_TOKEN.`);
      console.log('\nYou are now ready to run:');
      console.log('  npm run publish:chrome\n');
      process.exit(0);
    } catch (err) {
      console.error('❌ Error during token exchange:', err.message);
      process.exit(1);
    }
  }
});

server.listen(PORT, () => {
  try {
    const openCmd = process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'start' : 'xdg-open';
    execSync(`${openCmd} "${authUrl}"`);
  } catch {
    console.log('Please copy and paste the URL above into your browser manually.');
  }
});
