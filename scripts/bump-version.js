#!/usr/bin/env node

/**
 * Version bumper for WOL Highlights.
 * Updates wol-highlights/manifest.json, package.json, and Safari Xcode project.
 *
 * Usage:
 *   node scripts/bump-version.js [patch | minor | major | <explicit_version>]
 *   npm run bump          (defaults to patch)
 *   npm run bump:minor
 *   npm run bump:major
 */

const fs = require('fs');
const path = require('path');

const rootDir = path.resolve(__dirname, '..');
const manifestPath = path.join(rootDir, 'wol-highlights', 'manifest.json');
const pkgPath = path.join(rootDir, 'package.json');
const pbxprojPath = path.join(rootDir, 'safari', 'WOL Highlights', 'WOL Highlights.xcodeproj', 'project.pbxproj');

const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));

const currentVersion = manifest.version || pkg.version || '1.0.0';
const arg = process.argv[2] || 'patch';

let newVersion;

if (/^\d+\.\d+\.\d+$/.test(arg)) {
  newVersion = arg;
} else {
  const parts = currentVersion.split('.').map(Number);
  if (parts.length !== 3 || parts.some(isNaN)) {
    console.error(`Invalid current version format: ${currentVersion}`);
    process.exit(1);
  }

  let [major, minor, patch] = parts;

  switch (arg.toLowerCase()) {
    case 'major':
      major += 1;
      minor = 0;
      patch = 0;
      break;
    case 'minor':
      minor += 1;
      patch = 0;
      break;
    case 'patch':
    default:
      patch += 1;
      break;
  }

  newVersion = `${major}.${minor}.${patch}`;
}

console.log(`Bumping version: ${currentVersion} ➔ ${newVersion}`);

// 1. Update wol-highlights/manifest.json
manifest.version = newVersion;
fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n', 'utf8');
console.log('  ✓ Updated wol-highlights/manifest.json');

// 2. Update package.json
pkg.version = newVersion;
fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8');
console.log('  ✓ Updated package.json');

// 3. Update Safari Xcode project MARKETING_VERSION
if (fs.existsSync(pbxprojPath)) {
  try {
    let pbx = fs.readFileSync(pbxprojPath, 'utf8');
    pbx = pbx.replace(/MARKETING_VERSION = [^;]+;/g, `MARKETING_VERSION = ${newVersion};`);
    fs.writeFileSync(pbxprojPath, pbx, 'utf8');
    console.log('  ✓ Updated Safari Xcode project (MARKETING_VERSION)');
  } catch (err) {
    console.warn('  ⚠️ Could not update Xcode project version:', err.message);
  }
}

console.log(`\n🎉 Successfully bumped version to ${newVersion}!\n`);
