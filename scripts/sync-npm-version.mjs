#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const mainPackagePath = path.join(root, "npm", "cybersearch-mcp", "package.json");
const platformsDirectory = path.join(root, "npm", "platforms");
const checkOnly = process.argv.includes("--check");
const requestedVersion = process.argv.find((argument) => /^\d+\.\d+\.\d+/.test(argument));
const cargoVersion = fs
  .readFileSync(path.join(root, "Cargo.toml"), "utf8")
  .match(/^version = "([^"]+)"/m)?.[1];
const version = requestedVersion ?? cargoVersion;

if (!version) {
  throw new Error("Unable to determine version from argument or Cargo.toml");
}

const platformPackagePaths = fs
  .readdirSync(platformsDirectory, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => path.join(platformsDirectory, entry.name, "package.json"))
  .sort();
const packagePaths = [mainPackagePath, ...platformPackagePaths];
const platformNames = new Set(
  platformPackagePaths.map((packagePath) => JSON.parse(fs.readFileSync(packagePath, "utf8")).name),
);
const mainPackage = JSON.parse(fs.readFileSync(mainPackagePath, "utf8"));
const dependencyNames = new Set(Object.keys(mainPackage.optionalDependencies ?? {}));

if (
  platformNames.size !== dependencyNames.size ||
  [...platformNames].some((name) => !dependencyNames.has(name))
) {
  throw new Error("Main package optionalDependencies do not match the platform packages");
}

let changed = false;
for (const packagePath of packagePaths) {
  const packageJson = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  if (packageJson.version !== version) {
    changed = true;
    packageJson.version = version;
  }

  for (const dependency of Object.keys(packageJson.optionalDependencies ?? {})) {
    if (!platformNames.has(dependency)) {
      throw new Error(`Unknown platform dependency in ${packagePath}: ${dependency}`);
    }
    if (packageJson.optionalDependencies[dependency] !== version) {
      changed = true;
      packageJson.optionalDependencies[dependency] = version;
    }
  }

  if (!checkOnly) {
    fs.writeFileSync(packagePath, `${JSON.stringify(packageJson, null, 2)}\n`);
  }
}

if (checkOnly && changed) {
  throw new Error(`npm package versions are not synchronized with ${version}`);
}

console.log(`${checkOnly ? "verified" : "synchronized"}: ${packagePaths.length} npm packages @ ${version}`);
