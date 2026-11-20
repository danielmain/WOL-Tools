#!/usr/bin/env node

/**
 * iOS build/run helper for OfflineWOL (Tauri 2 mobile).
 *
 * Why this exists: on this machine `cargo`, `rustup` and `pod` are split across
 * ~/.cargo/bin, ~/.rustup/toolchains/<toolchain>/bin and /opt/homebrew/bin, and a Nix
 * devshell's cc-wrapper injects `-mmacos-version-min=14.0` (+ NIX_LDFLAGS),
 * which breaks iOS cross-compilation. This script assembles a clean environment
 * for `cargo tauri ios ...` so neither applies.
 *
 * Usage:
 *   npm run ios                 # cargo tauri ios dev on the first physical iOS device
 *   npm run ios:dev             # same
 *   npm run ios:sim             # cargo tauri ios dev on a booted/available simulator
 *   npm run ios:build           # signed .ipa (release, target aarch64)
 *   npm run ios:build:debug     # debug .ipa
 *   npm run ios:install         # install the newest .ipa on the device (no rebuild)
 *   npm run ios:launch          # launch com.offlinewol.desktop on the device
 *   npm run ios:run             # build + install + launch
 *   npm run ios:xcode           # `tauri ios dev --open` (CLI must keep running)
 *   npm run ios:open            # same, via `npm run ios -- --open`
 *   npm run ios:init            # (re)generate gen/apple via `tauri ios init`
 *
 * Flags (pass through `npm run ios -- <flags>`):
 *   --device <udid|name>   pick a specific device (substring match, or UDID prefix)
 *   --sim                  prefer an iOS simulator instead of a physical device
 *   --open                 open Xcode instead of running on the device
 *   --release|--debug      force the cargo profile
 *   --verbose / -v         forward to the tauri CLI
 *   --ci                   non-interactive (used by ios:build)
 */

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');
const DESKTOP = path.join(ROOT, 'wol-desktop');
const APPLE_DIR = path.join(DESKTOP, 'gen', 'apple');
const XCODE_PROJECT = path.join(APPLE_DIR, 'wol-desktop.xcodeproj');
const BUNDLE_ID = 'com.offlinewol.desktop';
const IPA_DIR = path.join(APPLE_DIR, 'build', 'arm64');

const COMMANDS = new Set(['dev', 'build', 'install', 'launch', 'run', 'xcode', 'init', 'help']);

const log = (msg) => process.stdout.write(`[ios] ${msg}\n`);
const die = (msg) => {
  process.stderr.write(`[ios] error: ${msg}\n`);
  process.exit(1);
};

// ---------------------------------------------------------------------------
// Toolchain / environment (pure)
// ---------------------------------------------------------------------------

/** rustup toolchain bin dir holding a real `cargo`, or undefined. */
const rustupBinDir = () => {
  const base = path.join(os.homedir(), '.rustup', 'toolchains');
  try {
    const found = fs
      .readdirSync(base)
      .map((name) => path.join(base, name, 'bin'))
      .find((bin) => fs.existsSync(path.join(bin, 'cargo')));
    return found;
  } catch {
    return undefined;
  }
};

const which = (bin) => {
  const dirs = (process.env.PATH || '').split(path.delimiter);
  return dirs.map((d) => path.join(d, bin)).find((p) => fs.existsSync(p));
};

/**
 * PATH with node, rustup, cargo/homebrew and the system dirs only.
 * Nix profile/prefixed bin dirs are deliberately dropped: their clang wrapper
 * breaks iOS cross-compilation.
 */
const cleanPath = () => {
  const entries = [
    // rustup toolchain first: the Homebrew `cargo` has no iOS std.
    rustupBinDir(),
    path.join(os.homedir(), '.cargo', 'bin'),
    path.dirname(process.execPath),
    path.join(os.homedir(), '.npm-packages', 'bin'),
    path.join(os.homedir(), '.nix-profile', 'bin'),
    '/opt/homebrew/bin',
    '/usr/local/bin',
    '/usr/bin',
    '/bin',
    '/usr/sbin',
    '/sbin',
  ].filter(Boolean);
  return [...new Set(entries)].join(path.delimiter);
};

const stripNixVars = (env) =>
  Object.fromEntries(
    Object.entries(env).filter(
      ([key]) => !key.startsWith('NIX_') && key !== 'SDKROOT' && key !== 'DEVELOPER_DIR',
    ),
  );

const IOS_TARGETS = ['aarch64_apple_ios', 'aarch64_apple_ios_sim', 'x86_64_apple_ios'];

/** Env for cross-compiling to iOS with the *system* clang, never Nix's wrapper. */
const iosEnv = (base = process.env) => ({
  ...stripNixVars(base),
  PATH: cleanPath(),
  CC: '/usr/bin/clang',
  CXX: '/usr/bin/clang++',
  CC_aarch64_apple_ios: '/usr/bin/clang',
  CXX_aarch64_apple_ios: '/usr/bin/clang++',
  CC_aarch64_apple_ios_sim: '/usr/bin/clang',
  CXX_aarch64_apple_ios_sim: '/usr/bin/clang++',
  CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER: '/usr/bin/clang',
  ...Object.fromEntries(
    IOS_TARGETS.flatMap((t) => [
      [`CARGO_TARGET_${t.toUpperCase()}_LINKER`, '/usr/bin/clang'],
    ]),
  ),
  // Rust std for iOS must come from the rustup toolchain, not a Nix toolchain.
  ...(rustupBinDir() ? { RUSTUP_TOOLCHAIN: path.basename(path.dirname(path.dirname(rustupBinDir()))) } : {}),
});

// ---------------------------------------------------------------------------
// Argument parsing (pure)
// ---------------------------------------------------------------------------

const parseArgs = (argv) => {
  const positional = [];
  const flags = { sim: false, open: false, release: undefined, ci: false, verbose: false, device: undefined, passthrough: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--device') {
      flags.device = argv[i + 1];
      i += 1;
    } else if (arg.startsWith('--device=')) {
      flags.device = arg.slice('--device='.length);
    } else if (arg === '--sim' || arg === '--simulator') {
      flags.sim = true;
    } else if (arg === '--open' || arg === '-o') {
      flags.open = true;
    } else if (arg === '--release') {
      flags.release = true;
    } else if (arg === '--debug') {
      flags.release = false;
    } else if (arg === '--ci') {
      flags.ci = true;
    } else if (arg === '--verbose' || arg === '-v') {
      flags.verbose = true;
    } else if (arg === '--help' || arg === '-h') {
      flags.help = true;
    } else if (arg === '--') {
      flags.passthrough = argv.slice(i + 1);
      break;
    } else if (COMMANDS.has(arg) && positional.length === 0) {
      positional.push(arg);
    } else if (!arg.startsWith('-')) {
      positional.push(arg);
    }
  }
  return { command: positional[0] || 'dev', flags };
};

// ---------------------------------------------------------------------------
// Device discovery (I/O at the edge)
// ---------------------------------------------------------------------------

const listDevices = () => {
  const out = path.join(os.tmpdir(), `wol-ios-devices-${process.pid}.json`);
  const run = spawnSync(
    'xcrun',
    ['devicectl', 'list', 'devices', '--json-output', out],
    { stdio: 'ignore', env: iosEnv() },
  );
  if (run.status !== 0 || !fs.existsSync(out)) return [];
  try {
    const data = JSON.parse(fs.readFileSync(out, 'utf8'));
    return (data.result?.devices || []).flatMap((dev) => {
      const udid = dev.hardwareProperties?.udid ?? dev.identifier;
      const platform = dev.hardwareProperties?.platform ?? dev.deviceProperties?.platform;
      if (!udid || platform !== 'iOS') return [];
      return [
        {
          udid,
          name: dev.deviceProperties?.name ?? dev.hardwareProperties?.name ?? udid,
          reality: dev.hardwareProperties?.reality ?? 'physical',
          os: dev.deviceProperties?.osVersionNumber ?? '?',
          lastSeen: Date.parse(dev.connectionProperties?.lastConnectionDate ?? '') || 0,
        },
      ];
    });
  } catch {
    return [];
  } finally {
    fs.rmSync(out, { force: true });
  }
};

/**
 * Deterministic device preference: iPhone before iPad (iPadOS devices are often
 * idle/locked), then the most recently connected. devicectl lists arbitrarily.
 */
const byPreference = (a, b) => {
  const phone = (d) => (/iphone/i.test(d.name) ? 0 : 1);
  return phone(a) - phone(b) || b.lastSeen - a.lastSeen || a.name.localeCompare(b.name);
};

const pickDevice = (devices, { query, sim }) => {
  const wanted = devices
    .filter((d) => (sim ? d.reality === 'simulated' : d.reality !== 'simulated'))
    .sort(byPreference);
  if (wanted.length === 0) {
    return undefined;
  }
  if (!query) return wanted[0];
  const q = query.toLowerCase();
  return (
    wanted.find((d) => d.udid.toLowerCase() === q) ||
    wanted.find((d) => d.udid.toLowerCase().startsWith(q)) ||
    wanted.find((d) => d.name.toLowerCase().includes(q))
  );
};

const resolveDevice = (flags) => {
  const devices = listDevices();
  const device = pickDevice(devices, { query: flags.device, sim: flags.sim });
  if (!device) {
    const listing = devices.map((d) => `  ${d.reality === 'simulated' ? 'sim' : 'dev'} ${d.udid}  ${d.name} (iOS ${d.os})`).join('\n');
    die(
      `no ${flags.sim ? 'simulator' : 'physical iOS device'} found` +
        (flags.device ? ` matching "${flags.device}"` : '') +
        (listing ? `\nconnected:\n${listing}` : '\n(no iOS devices reported by devicectl)') +
        '\n       → plug in / unlock the iPhone, or pass --device <udid>',
    );
  }
  return device;
};

const resolveDeviceOrUndefined = (flags) =>
  pickDevice(listDevices(), { query: flags.device, sim: flags.sim });

/** Newest *.ipa produced by `tauri ios build`, or undefined. */
const newestIpa = () => {
  try {
    const ipas = fs
      .readdirSync(IPA_DIR)
      .filter((name) => name.endsWith('.ipa'))
      .map((name) => path.join(IPA_DIR, name))
      .map((p) => ({ p, mtime: fs.statSync(p).mtimeMs }))
      .sort((a, b) => b.mtime - a.mtime);
    return ipas[0]?.p;
  } catch {
    return undefined;
  }
};

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

/** DerivedData project dirs for this app (Xcode hashes the project path). */
const derivedDataDirs = () => {
  const base = path.join(os.homedir(), 'Library', 'Developer', 'Xcode', 'DerivedData');
  try {
    return fs
      .readdirSync(base)
      .filter((name) => name.startsWith('wol-desktop-'))
      .map((name) => path.join(base, name));
  } catch {
    return [];
  }
};

const buildDbPaths = () =>
  derivedDataDirs()
    .map((dir) => path.join(dir, 'Build', 'Intermediates.noindex', 'XCBuildData', 'build.db'))
    .filter((file) => fs.existsSync(file));

const buildDbHolders = (file) => {
  const probe = spawnSync('lsof', ['-t', file], { encoding: 'utf8' });
  return (probe.stdout || '')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
};

/**
 * Xcode allows a single writer on DerivedData's build.db. An Xcode GUI build
 * (which never spawns an `xcodebuild` process) or a parallel CLI build holds it;
 * a second build then dies with "database is locked". Refuse up front instead.
 */
const assertNoConcurrentBuild = () => {
  const busy = buildDbPaths().filter((file) => buildDbHolders(file).length > 0);
  if (busy.length === 0) return;
  die(
    'Xcode is already building this project (build.db is locked by another process):\n' +
      `       ${busy[0]}\n` +
      '       Stop/Xcode-cancel that build first, or use Xcode\'s own Run button.',
  );
};

const run = (cmd, args, options = {}) => {
  log(`$ ${[cmd, ...args].join(' ')}`);
  const result = spawnSync(cmd, args, {
    cwd: DESKTOP,
    stdio: 'inherit',
    env: iosEnv(),
    ...options,
  });
  if (result.error) die(`${cmd}: ${result.error.message}`);
  return result.status ?? 1;
};

const requireTauri = () => {
  const env = iosEnv();
  const hasCargoTauri =
    fs.existsSync(path.join(os.homedir(), '.cargo', 'bin', 'cargo-tauri')) ||
    Boolean(which('cargo-tauri'));
  if (!hasCargoTauri) {
    die('cargo-tauri is not installed → `cargo install tauri-cli --version "^2"`');
  }
  const probe = spawnSync('cargo', ['tauri', '--version'], { env, stdio: 'ignore' });
  if (probe.status !== 0) die('`cargo tauri` is not runnable in the cleaned environment');
};

const ensureProject = () => {
  if (fs.existsSync(XCODE_PROJECT)) return;
  log('gen/apple missing → running `tauri ios init --ci --skip-targets-install`');
  const status = run('cargo', ['tauri', 'ios', 'init', '--ci', '--skip-targets-install']);
  if (status !== 0) die('`tauri ios init` failed');
};

const simRuntimeHint = () => {
  const sdk = spawnSync('xcodebuild', ['-showsdks'], { encoding: 'utf8', env: iosEnv() });
  const sdkMatch = /Simulator - iOS ([\d.]+)/.exec(sdk.stdout || '');
  const runtimes = spawnSync('xcrun', ['simctl', 'list', 'runtimes'], { encoding: 'utf8', env: iosEnv() });
  const available = [...(runtimes.stdout || '').matchAll(/iOS ([\d.]+) /g)].map((m) => m[1]);
  if (sdkMatch && available.length > 0 && !available.some((v) => sdkMatch[1].startsWith(v) || v.startsWith(sdkMatch[1]))) {
    log(
      `note: Xcode has the iphonesimulator${sdkMatch[1]} SDK but simctl only has iOS ${available.join(', ')} runtimes.\n` +
        `      Download the matching runtime in Xcode → Settings → Components, or use a physical device (--device).`,
    );
  }
};

const cmdDev = (flags) => {
  ensureProject();
  requireTauri();
  assertNoConcurrentBuild();
  if (flags.sim) simRuntimeHint();
  const args = ['tauri', 'ios', 'dev'];
  if (flags.release === true) args.push('--release');
  if (flags.verbose) args.push('-vv');
  if (flags.open) {
    // --open hands the build to Xcode while the CLI keeps writing
    // gen/apple/.tauri/cli-options-server.json (Xcode's "Build Rust Code" phase
    // reads it and fails without a running `tauri ios dev`).
    args.push('--open');
    log('opening Xcode; keep this process running while Xcode builds');
  } else {
    const device = resolveDeviceOrUndefined(flags);
    if (device) log(`device → ${device.name} (iOS ${device.os})`);
    if (device && device.reality !== 'simulated') args.push(device.name);
  }
  const status = run('cargo', [...args, ...flags.passthrough]);
  if (status !== 0 && !flags.open) {
    log(
      'hints: unlock the device and keep it cabled, or pick another one with --device <udid>;\n' +
        '      a build started inside Xcode.app needs `npm run ios:xcode` to be running.',
    );
  }
  return status;
};

const cmdBuild = (flags) => {
  ensureProject();
  requireTauri();
  assertNoConcurrentBuild();
  const args = ['tauri', 'ios', 'build', '--target', 'aarch64'];
  if (flags.release === false) args.push('--debug');
  if (flags.verbose) args.push('-vv');
  args.push('--ci');
  const status = run('cargo', [...args, ...flags.passthrough]);
  if (status === 0) {
    const ipa = newestIpa();
    if (ipa) log(`ipa → ${path.relative(ROOT, ipa)}`);
  }
  return status;
};

const cmdInstall = (flags) => {
  const ipa = newestIpa();
  if (!ipa) die('no .ipa found → run `npm run ios:build` first');
  const device = resolveDevice({ ...flags, sim: false });
  log(`installing ${path.relative(ROOT, ipa)} → ${device.name} (${device.udid})`);
  return run('xcrun', ['devicectl', 'device', 'install', 'app', '--device', device.udid, ipa], {
    cwd: ROOT,
  });
};

const cmdLaunch = (flags) => {
  const device = resolveDevice({ ...flags, sim: false });
  log(`launching ${BUNDLE_ID} → ${device.name}`);
  const status = run('xcrun', ['devicectl', 'device', 'process', 'launch', '--device', device.udid, BUNDLE_ID], {
    cwd: ROOT,
  });
  if (status !== 0) log('if the error mentions "Locked", unlock the iPhone and retry');
  return status;
};

const cmdRun = (flags) => {
  const build = cmdBuild(flags);
  if (build !== 0) return build;
  const install = cmdInstall(flags);
  if (install !== 0) return install;
  return cmdLaunch(flags);
};

const cmdXcode = () => {
  ensureProject();
  requireTauri();
  // Opening the .xcodeproj alone is not enough: Xcode's "Build Rust Code" phase
  // reads gen/apple/.tauri/cli-options-server.json, which only a *running*
  // `tauri ios dev` writes. --open starts the CLI and opens Xcode.
  log('starting `tauri ios dev --open` — keep this process running while Xcode builds');
  return run('cargo', ['tauri', 'ios', 'dev', '--open']);
};

const cmdInit = () => {
  requireTauri();
  return run('cargo', ['tauri', 'ios', 'init', '--ci', '--skip-targets-install']);
};

const usage = () => {
  process.stdout.write(
    [
      'Usage: npm run ios[:dev|:sim|:build|:install|:launch|:run|:xcode|:init] -- [flags]',
      '',
      '  dev       cargo tauri ios dev on a physical device (default)',
      '  build     signed .ipa → wol-desktop/gen/apple/build/arm64/',
      '  install   install the newest .ipa via devicectl',
      '  launch    launch com.offlinewol.desktop on the device',
      '  run       build + install + launch',
      '  xcode     open Xcode via `tauri ios dev --open` (keep the CLI running)',
      '  init      regenerate gen/apple',
      '',
      'Flags: --device <udid|name>  --sim  --open  --release  --debug  --verbose  --ci',
      '',
    ].join('\n'),
  );
  return 0;
};

const main = () => {
  const { command, flags } = parseArgs(process.argv.slice(2));
  if (flags.help) return usage();
  switch (command) {
    case 'dev':
      return cmdDev(flags);
    case 'build':
      return cmdBuild(flags);
    case 'install':
      return cmdInstall(flags);
    case 'launch':
      return cmdLaunch(flags);
    case 'run':
      return cmdRun(flags);
    case 'xcode':
      return cmdXcode(flags);
    case 'init':
      return cmdInit(flags);
    default:
      return usage();
  }
};

process.exit(main());
