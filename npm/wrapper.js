#!/usr/bin/env node
const { spawnSync } = require('child_process');
const os = require('os');

const platform = os.platform();
const arch = os.arch();

const pkgName = platform === 'win32' ? '@dzulfikarts/deslop-rs-win32-x64' : `deslop-rs-${platform}-${arch}`;
const ext = platform === 'win32' ? '.exe' : '';
let binaryPath;
try {
  binaryPath = require.resolve(`${pkgName}/bin/deslop${ext}`);
} catch {
  console.error(`deslop-rs: could not find a deslop binary for ${platform}-${arch}.`);
  console.error('Supported: darwin-arm64, darwin-x64, linux-arm64, linux-x64, win32-x64.');
  console.error(`If you are on a supported platform, try reinstalling so the optional package '${pkgName}' is installed.`);
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: 'inherit', shell: false });
if (result.error) {
  console.error(`deslop-rs: failed to start: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
