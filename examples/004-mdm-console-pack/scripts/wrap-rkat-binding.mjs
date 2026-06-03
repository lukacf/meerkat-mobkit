#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const [, , inputArg, outputArg, metadataArg] = process.argv;
if (!inputArg || !outputArg || !metadataArg) {
  console.error("Usage: wrap-rkat-binding.mjs <rkat-binding.json> <output.json> <metadata-json>");
  process.exit(2);
}

const input = resolve(inputArg);
const output = resolve(outputArg);
const binding = JSON.parse(readFileSync(input, "utf8"));
const metadata = JSON.parse(metadataArg);

if (!metadata.id) {
  throw new Error("metadata must include id");
}

const wrapped = {
  ...metadata,
  address: metadata.address ?? binding.address,
  binding: {
    ...binding,
    address: metadata.address ?? binding.address,
  },
};

mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, `${JSON.stringify(wrapped, null, 2)}\n`);
