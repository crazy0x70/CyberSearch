#!/usr/bin/env node

"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

const PLATFORM_PACKAGES = {
  "darwin-x64": "cybersearch-mcp-darwin-x64",
  "darwin-arm64": "cybersearch-mcp-darwin-arm64",
  "linux-x64": "cybersearch-mcp-linux-x64",
  "linux-arm64": "cybersearch-mcp-linux-arm64",
  "win32-x64": "cybersearch-mcp-win32-x64",
  "win32-arm64": "cybersearch-mcp-win32-arm64",
};

function resolveBinary() {
  if (process.env.CYBERSEARCH_BINARY) {
    return path.resolve(process.env.CYBERSEARCH_BINARY);
  }

  const platformKey = `${process.platform}-${process.arch}`;
  const platformPackage = PLATFORM_PACKAGES[platformKey];
  if (!platformPackage) {
    throw new Error(
      `Unsupported platform: ${platformKey}. Supported: ${Object.keys(PLATFORM_PACKAGES).join(", ")}`,
    );
  }

  try {
    const packageJson = require.resolve(`${platformPackage}/package.json`);
    const executable = process.platform === "win32" ? "cybersearch.exe" : "cybersearch";
    return path.join(path.dirname(packageJson), "bin", executable);
  } catch (_) {
    throw new Error(
      `Platform package not found: ${platformPackage}. Reinstall with: npm install -g cybersearch-mcp`,
    );
  }
}

function main() {
  let binary;
  try {
    binary = resolveBinary();
  } catch (error) {
    console.error(`CyberSearch: ${error.message}`);
    process.exit(1);
  }

  if (!fs.existsSync(binary)) {
    console.error(`CyberSearch: prebuilt binary not found: ${binary}`);
    console.error("Reinstall with: npm install -g cybersearch-mcp");
    process.exit(1);
  }

  const child = spawn(binary, process.argv.slice(2), {
    env: process.env,
    stdio: "inherit",
  });

  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, () => {
      if (!child.killed) child.kill(signal);
    });
  }

  child.on("error", (error) => {
    console.error(`CyberSearch: failed to start native binary: ${error.message}`);
    process.exit(1);
  });

  child.on("exit", (code, signal) => {
    if (signal) {
      process.exit(128 + (os.constants.signals[signal] || 0));
    }
    process.exit(code ?? 1);
  });
}

main();
