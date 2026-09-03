#!/usr/bin/env node
// Build a per-platform Claude Desktop extension (.mcpb) for hetzner-mcp.
//
// Cross-platform (runs on Linux, macOS, and Windows via Node). Builds the
// release binary for the host platform, stages it with a patched manifest and
// the committed icon, and packs:
//
//   dist/hetzner-mcp-<os>.mcpb      where <os> is linux | macos | windows
//
// The manifest's version is read from Cargo.toml (single source of truth, so it
// can never drift from the crate), and entry_point / command / platforms are
// set for the target. On macOS a universal (arm64 + x86_64) binary is produced
// via lipo when both targets are installed; otherwise the native arch is used.
//
// Usage:  node scripts/build-mcpb.mjs
//         node scripts/build-mcpb.mjs --sync-manifest   (refresh mcpb/manifest.json's
//                                                         tools + version from a live
//                                                         debug build; run + commit
//                                                         whenever a tool description changes)
// Requires: cargo, node/npx. macOS universal builds also need lipo (Xcode CLT).

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const BIN_NAME = "hetzner-mcp";
// Version-pinned: this tool validates and packs the shipped artifact, so it is
// frozen the same way the workflow's third-party actions are SHA-pinned.
const MCPB_CLI = "@anthropic-ai/mcpb@2.1.2";

// Map Node's process.platform to a friendly OS slug + manifest values.
//
// `target` is a rustc triple the binary is built for instead of the host default:
// Linux ships the static musl build so the .mcpb runs on any distro regardless of
// the host's glibc version (a glibc binary carries a floor set by the build box).
// Requires `rustup target add x86_64-unknown-linux-musl`; the rustls TLS stack
// (see Cargo.toml) means no OpenSSL to cross-link into the static binary.
const PLATFORMS = {
  linux: { slug: "linux", nodePlatform: "linux", exe: "", target: "x86_64-unknown-linux-musl" },
  darwin: { slug: "macos", nodePlatform: "darwin", exe: "" },
  win32: { slug: "windows", nodePlatform: "win32", exe: ".exe" },
};

const platform = PLATFORMS[process.platform];
if (!platform) {
  console.error(`Unsupported platform: ${process.platform}`);
  process.exit(1);
}

const run = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { cwd: ROOT, stdio: "inherit", shell: false, ...opts });

// `npx` is the `npx.cmd` batch shim on Windows, which can only be launched via a
// shell. All args here are trusted static paths, so shell quoting is not a concern.
const npx = (args) => run("npx", args, { shell: process.platform === "win32" });

// Single source of truth for the version: Cargo.toml [package] version.
function crateVersion() {
  const toml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const m = toml.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error("could not read version from Cargo.toml");
  return m[1];
}

// D2 finding 2: regenerate the committed manifest's `tools` array + `version`
// from a live binary instead of hand-copying tool descriptions, which drifts
// the moment a tool's description changes. Run via `--sync-manifest`; not
// part of the per-platform release build below (same tool list on every
// platform, so redoing it 3x in CI would be pure waste).
function toolsFromLiveBinary() {
  console.log("==> Building a debug binary to introspect tools");
  run("cargo", ["build", "--locked"]);
  const binPath = join(ROOT, "target", "debug", `${BIN_NAME}${platform.exe}`);
  // The binary reads projects from a config file (no env vars), so hand it a
  // throwaway one via --config. Synthetic - never a real token - and
  // single-project, so no `project` schema property leaks into the committed
  // tool descriptions.
  const token = "a".repeat(64);
  const tmpDir = mkdtempSync(join(tmpdir(), "hetzner-mcp-manifest-"));
  const cfgPath = join(tmpDir, "config.toml");
  writeFileSync(cfgPath, `[[projects]]\nname = "sync"\ntoken = "${token}"\n`, { mode: 0o600 });
  const requests =
    [
      {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2026-07-28",
          capabilities: {},
          clientInfo: { name: "mcpb-manifest-sync", version: "0" },
        },
      },
      { jsonrpc: "2.0", method: "notifications/initialized" },
      { jsonrpc: "2.0", id: 2, method: "tools/list" },
    ]
      .map((m) => JSON.stringify(m))
      .join("\n") + "\n";
  let output;
  try {
    output = execFileSync(binPath, ["--config", cfgPath], {
      cwd: ROOT,
      input: requests,
      encoding: "utf8",
    });
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
  const listResult = output
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line))
    .find((msg) => msg.id === 2)?.result;
  if (!listResult) throw new Error("binary did not answer tools/list on stdio");
  return listResult.tools
    .map((t) => ({ name: t.name, description: t.description }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

function syncManifest() {
  const manifestPath = join(ROOT, "mcpb", "manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifest.tools = toolsFromLiveBinary();
  manifest.version = crateVersion();
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
  console.log(`==> mcpb/manifest.json synced: ${manifest.tools.length} tools, v${manifest.version}`);
}

// Build the release binary for the host. On macOS, attempt a universal binary.
function buildBinary(stageBinDir) {
  const out = join(stageBinDir, `${BIN_NAME}${platform.exe}`);
  if (process.platform === "darwin") {
    const targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
    console.log("==> Building universal macOS binary");
    for (const t of targets) {
      run("rustup", ["target", "add", t]);
      run("cargo", ["build", "--release", "--locked", "--target", t]);
    }
    run("lipo", [
      "-create", "-output", out,
      join(ROOT, "target", targets[0], "release", BIN_NAME),
      join(ROOT, "target", targets[1], "release", BIN_NAME),
    ]);
    run("lipo", ["-info", out]);
  } else if (platform.target) {
    console.log(`==> Building release binary for ${platform.target}`);
    run("rustup", ["target", "add", platform.target]);
    run("cargo", ["build", "--release", "--locked", "--target", platform.target]);
    copyFileSync(join(ROOT, "target", platform.target, "release", `${BIN_NAME}${platform.exe}`), out);
  } else {
    console.log("==> Building release binary");
    run("cargo", ["build", "--release", "--locked"]);
    copyFileSync(join(ROOT, "target", "release", `${BIN_NAME}${platform.exe}`), out);
  }
}

function main() {
  const version = crateVersion();
  const distDir = join(ROOT, "dist");
  const stageDir = join(distDir, `stage-${platform.slug}`);
  const stageBinDir = join(stageDir, "bin");
  const outFile = join(distDir, `${BIN_NAME}-${platform.slug}.mcpb`);

  console.log(`==> Packing ${BIN_NAME} v${version} for ${platform.slug}`);

  rmSync(stageDir, { recursive: true, force: true });
  mkdirSync(stageBinDir, { recursive: true });
  mkdirSync(distDir, { recursive: true });

  // Patch the committed base manifest for this target. Keeping it fresh
  // (tools + version) is `--sync-manifest`'s job, not this build's (W3.7,
  // D2 finding 2) - entry_point/command/platforms below are per-target and
  // stay in the staged copy only.
  const manifestPath = join(ROOT, "mcpb", "manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifest.version = version;
  manifest.server.entry_point = `bin/${BIN_NAME}${platform.exe}`;
  manifest.server.mcp_config.command = `\${__dirname}/bin/${BIN_NAME}${platform.exe}`;
  manifest.compatibility = manifest.compatibility || {};
  manifest.compatibility.platforms = [platform.nodePlatform];

  writeFileSync(join(stageDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  copyFileSync(join(ROOT, "mcpb", "icon.png"), join(stageDir, "icon.png"));

  buildBinary(stageBinDir);

  console.log("==> Validating manifest");
  npx(["-y", MCPB_CLI, "validate", join(stageDir, "manifest.json")]);

  console.log("==> Packing .mcpb");
  npx(["-y", MCPB_CLI, "pack", stageDir, outFile]);

  rmSync(stageDir, { recursive: true, force: true });
  console.log(`==> Done: ${outFile}`);
}

if (process.argv.includes("--sync-manifest")) {
  syncManifest();
} else {
  main();
}
