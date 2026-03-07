#!/usr/bin/env node

import { cargoPrepare } from "./release-common.mjs";

const version = process.argv[2];
if (!version) {
  throw new Error("usage: release-cargo-prepare.mjs <version>");
}

cargoPrepare(version);
