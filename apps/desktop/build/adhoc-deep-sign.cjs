// Ad-hoc deep code-signing hook for the macOS .app bundle.
//
// electron-builder calls this automatically after packing the app and BEFORE
// building the DMG, so the resulting DMG ships a validly-signed app. Ad-hoc
// signing (--sign -) creates a signature that lets users run the app via
// right-click → Open (or System Settings → Privacy & Security → "Open Anyway")
// instead of the "damaged and can't be opened" quarantine unsigned apps get.
//
// This requires NO Apple Developer ID and NO notarization. When a Developer ID
// is available, set CSC_LINK/CSC_KEY_PASSWORD (or CSC_NAME/CSC_KEYCHAIN) and this
// hook skips itself so electron-builder's built-in Developer ID signing runs.
//
// Ported from the proven pattern in the MetalSharp project.
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", stdio: "inherit" });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with status ${result.status}`);
  }
}

function shouldSkipAdhocSigning() {
  if (process.env.GRAPHIQ_SKIP_ADHOC_DEEP_SIGN === "1") {
    return "GRAPHIQ_SKIP_ADHOC_DEEP_SIGN=1";
  }
  // If Developer ID signing credentials are present, let electron-builder do
  // real signing instead — this hook would only undo/conflict with it.
  const developerIdSigning =
    Boolean(process.env.CSC_KEYCHAIN) || Boolean(process.env.CSC_LINK) || Boolean(process.env.CSC_NAME);
  if (developerIdSigning && process.env.GRAPHIQ_UNSIGNED_DMG !== "1") {
    return "Developer ID signing is active";
  }
  return "";
}

function isMachO(filePath) {
  if (!fs.statSync(filePath).isFile()) {
    return false;
  }
  const output = spawnSync("file", ["-b", filePath], { encoding: "utf8" });
  return (output.stdout || "").includes("Mach-O");
}

function collectSignTargets(root) {
  const files = [];
  const bundles = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const fullPath = path.join(current, entry.name);
      if (entry.isSymbolicLink()) {
        continue;
      }
      if (entry.isDirectory()) {
        if (/\.(app|appex|framework|xpc)$/i.test(entry.name)) {
          bundles.push(fullPath);
        }
        stack.push(fullPath);
      } else if (entry.isFile() && isMachO(fullPath)) {
        files.push(fullPath);
      }
    }
  }
  // Sign deepest-first so nested bundles get a valid parent signature.
  bundles.sort((a, b) => b.split(path.sep).length - a.split(path.sep).length);
  files.sort((a, b) => b.split(path.sep).length - a.split(path.sep).length);
  return { files, bundles };
}

function signTarget(target) {
  run("codesign", ["--force", "--sign", "-", "--timestamp=none", target]);
}

exports.default = async function adhocDeepSignGraphiq(context) {
  if (context.electronPlatformName !== "darwin") {
    return;
  }
  const skipReason = shouldSkipAdhocSigning();
  if (skipReason) {
    console.log(`GraphIQ ad-hoc deep sign skipped: ${skipReason}.`);
    return;
  }
  const appName = context.packager.appInfo.productFilename;
  const appPath = path.join(context.appOutDir, `${appName}.app`);
  if (!fs.existsSync(appPath)) {
    throw new Error(`GraphIQ app bundle was not found for ad-hoc signing: ${appPath}`);
  }
  const { files, bundles } = collectSignTargets(appPath);
  console.log(`GraphIQ ad-hoc deep sign: ${files.length} Mach-O file(s), ${bundles.length} bundle(s).`);
  for (const file of files) {
    signTarget(file);
  }
  for (const bundle of bundles) {
    signTarget(bundle);
  }
  // Final top-level signature on the .app itself.
  run("codesign", ["--force", "--deep", "--strict", "--sign", "-", "--timestamp=none", appPath]);
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath]);
  console.log("GraphIQ ad-hoc deep sign complete.");
};
