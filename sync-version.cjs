const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

// 1. Grab the freshly updated version from package.json
const packageJson = require('./package.json');
const version = packageJson.version;

console.log(`\n🚀 Syncing desktop engine targets to v${version}...`);

// 2. Update src-tauri/tauri.conf.json
const tauriConfPath = path.join(__dirname, 'src-tauri', 'tauri.conf.json');
if (fs.existsSync(tauriConfPath)) {
  const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, 'utf8'));
  tauriConf.version = version;
  fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2), 'utf8');
  console.log('  ✅ Updated tauri.conf.json');
}

// 3. Update src-tauri/Cargo.toml
const cargoPath = path.join(__dirname, 'src-tauri', 'Cargo.toml');
if (fs.existsSync(cargoPath)) {
  let cargoContent = fs.readFileSync(cargoPath, 'utf8');
  // Replaces the first instance of version = "X.Y.Z" (under [package])
  cargoContent = cargoContent.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
  fs.writeFileSync(cargoPath, cargoContent, 'utf8');
  console.log('  ✅ Updated Cargo.toml');
  
  // CRITICAL FIX: Sync Cargo.lock with the new Cargo.toml version
  try {
    execSync('cargo update -p heriheri', { cwd: path.join(__dirname, 'src-tauri'), stdio: 'ignore' });
    console.log('  ✅ Synced Cargo.lock');
  } catch (e) {
    console.warn('  ⚠️ Could not update Cargo.lock automatically. You may need to run cargo check manually.');
  }
}

// 3.5 Update openwrt-daemon/Cargo.toml
const openwrtCargoPath = path.join(__dirname, 'openwrt-daemon', 'Cargo.toml');
if (fs.existsSync(openwrtCargoPath)) {
  let cargoContent = fs.readFileSync(openwrtCargoPath, 'utf8');
  // Replaces the first instance of version = "X.Y.Z" (under [package])
  cargoContent = cargoContent.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
  fs.writeFileSync(openwrtCargoPath, cargoContent, 'utf8');
  console.log('  ✅ Updated openwrt-daemon/Cargo.toml');
  
  // Sync Cargo.lock for OpenWRT
  try {
    execSync('cargo update -p heriheri-openwrt', { cwd: path.join(__dirname, 'openwrt-daemon'), stdio: 'ignore' });
    console.log('  ✅ Synced openwrt-daemon/Cargo.lock');
  } catch (e) {
    console.warn('  ⚠️ Could not update openwrt-daemon/Cargo.lock automatically.');
  }
}

// 4. Force Git to stage these changes so they get bundled into the automatic version commit
try {
  // Added Cargo.lock to the git add command
  execSync('git add src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock openwrt-daemon/Cargo.toml');
  console.log('  ✅ Staged updated files for Git commit\n');
} catch (e) {
  console.error('  ❌ Failed to automatically stage files in Git:', e.message);
}