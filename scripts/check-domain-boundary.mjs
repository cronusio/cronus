#!/usr/bin/env node
// Boundary guard: fails the build if cronus-domain gains a normal dependency
// outside its allowlist. The crate split enforces the tier model at compile
// time only if this check runs on every build — otherwise the layout drifts
// exactly as it did before the split, silently.
//
// The adapter crates — cronus-store-local, cronus-auth-local, and
// cronus-model-local (the model-transport adapter) — are deliberately ABSENT
// from the allowlist: domain reaches them only through the contract traits
// (`UserDataStore`, `AuthProvider`, `InferenceBackend`, …), never by a direct
// dependency. A `cronus-domain -> cronus-model-local` edge is exactly the
// inward-dependency drift (INV-8) this guard exists to reject.
//
// A second check keeps the tier from drifting further from its target of doing
// no I/O. The manifest check cannot see a `std::fs` call, so the guard also
// scans the production part of every domain source file for filesystem,
// process and socket calls and compares the files that do against a committed
// baseline (`scripts/domain-io-baseline.json`). The baseline only shrinks: a
// file outside it that gains such a call fails the build, and a file that no
// longer needs its entry must drop it. Moving a module's I/O behind a seam
// therefore ratchets the tier toward I/O-free, and nothing can quietly move it
// the other way.
//
// Run `--self-test` to exercise the failure paths themselves: it asserts the
// real tree is clean AND that an injected `domain -> model-local` edge and an
// injected I/O drift are flagged, so the guard's rejection logic is proven, not
// just assumed. `--print-io-files` prints the current I/O file list as JSON
// (the form the baseline is stored in).

import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DOMAIN_SRC = path.join(REPO_ROOT, "crates", "domain", "src");
const IO_BASELINE = path.join(REPO_ROOT, "scripts", "domain-io-baseline.json");

// Calls that reach outside the process: the local filesystem, a child process,
// a socket.
const IO_PATTERN =
    /\bstd::fs\b|\bfs::(?:read|write|create_dir|remove|rename|File|OpenOptions|read_dir|metadata|copy)|\bFile::(?:open|create)|\bOpenOptions\b|\bTcpStream\b|\bstd::net::|\bstd::process::Command\b|\bCommand::new\b/;

// serde/serde_json (record (de)serialization over caller-supplied text) and the
// `tracing` emission facade are admitted: neither opens anything itself. The
// subscriber side (`tracing-subscriber`, `tracing-appender`) is infrastructure
// and stays out.
const ALLOWED = new Set([
    "cronus-contract",
    "nodus",
    "blake3",
    "chrono",
    "cron",
    "serde",
    "serde_json",
    "tracing",
]);

// The adapter crates domain must never depend on directly (checked by the
// self-test to stay off the allowlist).
const FORBIDDEN_ADAPTERS = ["cronus-store-local", "cronus-auth-local", "cronus-model-local"];

// Pure core, separated from I/O so the self-test can drive it with synthetic
// inputs: given cronus-domain's normal-dependency names, return the ones
// outside the allowlist.
function findOffenders(depNames, allowed = ALLOWED) {
    return depNames.filter((name) => !allowed.has(name));
}

// Read cronus-domain's resolved normal (non-dev, non-build) dependencies from
// cargo metadata.
function domainNormalDeps() {
    const raw = execFileSync("cargo", ["metadata", "--format-version=1"], {
        maxBuffer: 1024 * 1024 * 64,
    }).toString("utf8");
    const metadata = JSON.parse(raw);

    const domainPkg = metadata.packages.find((p) => p.name === "cronus-domain");
    if (!domainPkg) {
        throw new Error("no package named cronus-domain in the workspace metadata");
    }
    const namesById = new Map(metadata.packages.map((p) => [p.id, p.name]));
    const node = metadata.resolve.nodes.find((n) => n.id === domainPkg.id);

    return node.deps
        .filter((d) => d.dep_kinds.some((k) => k.kind === null))
        .map((d) => namesById.get(d.pkg));
}

// Whether a source file's production code does I/O. Everything from the first
// `#[cfg(test)]` on is test code and is not counted; comment lines are skipped.
function fileDoesIo(text) {
    const lines = text.split("\n");
    const cut = lines.findIndex((line) => line.trim() === "#[cfg(test)]");
    const production = cut === -1 ? lines : lines.slice(0, cut);
    return production.some((line) => !line.trim().startsWith("//") && IO_PATTERN.test(line));
}

// The domain source files whose production code does I/O, as sorted paths
// relative to `crates/domain/src` with forward slashes.
function domainIoFiles(dir = DOMAIN_SRC, root = DOMAIN_SRC) {
    const found = [];
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            found.push(...domainIoFiles(full, root));
        } else if (entry.isFile() && entry.name.endsWith(".rs")) {
            if (fileDoesIo(readFileSync(full, "utf8"))) {
                found.push(path.relative(root, full).split(path.sep).join("/"));
            }
        }
    }
    return found.sort();
}

// Pure core of the I/O ratchet: files that do I/O but are not in the baseline
// (new drift), and baseline entries that no longer do I/O (the baseline must
// shrink).
function ioDrift(current, baseline) {
    const base = new Set(baseline);
    const now = new Set(current);
    return {
        added: current.filter((file) => !base.has(file)),
        stale: baseline.filter((file) => !now.has(file)),
    };
}

function readIoBaseline() {
    return JSON.parse(readFileSync(IO_BASELINE, "utf8"));
}

// Exercise the guard's own failure path so "the boundary is enforced" is a
// tested fact, not a claim.
function selfTest() {
    const realDeps = domainNormalDeps();

    // 1. The real tree is clean.
    assert.deepEqual(
        findOffenders(realDeps),
        [],
        `the real cronus-domain deps must be within the allowlist; offenders: ${findOffenders(realDeps).join(", ")}`,
    );

    // 2. An injected `domain -> cronus-model-local` edge is rejected — the
    //    exact drift this guard must catch.
    const injected = [...realDeps, "cronus-model-local"];
    assert.ok(
        findOffenders(injected).includes("cronus-model-local"),
        "an injected domain->model-local edge must be flagged as a boundary violation",
    );

    // 3. No adapter crate is ever on the allowlist (belt and suspenders — a
    //    future edit adding one would silently defeat the guard).
    for (const adapter of FORBIDDEN_ADAPTERS) {
        assert.ok(!ALLOWED.has(adapter), `${adapter} must not be on the domain allowlist`);
    }

    // 4. The subscriber side of logging is infrastructure: an injected
    //    `tracing-subscriber` or `tracing-appender` edge is rejected even
    //    though the `tracing` facade itself is admitted.
    for (const subscriber of ["tracing-subscriber", "tracing-appender"]) {
        assert.ok(
            findOffenders([...realDeps, subscriber]).includes(subscriber),
            `${subscriber} must be flagged as a boundary violation`,
        );
    }

    // 5. The I/O ratchet: the real tree matches its baseline, production code is
    //    told apart from test code, comments do not count, and both directions of
    //    drift are flagged.
    const realDrift = ioDrift(domainIoFiles(), readIoBaseline());
    assert.deepEqual(realDrift.added, [], `domain files gained I/O outside the baseline: ${realDrift.added.join(", ")}`);
    assert.deepEqual(realDrift.stale, [], `baseline entries no longer do I/O: ${realDrift.stale.join(", ")}`);
    assert.ok(fileDoesIo("use std::fs;\nfn read() {}"), "a std::fs use in production code must count as I/O");
    assert.ok(fileDoesIo("fn run() { Command::new(\"x\"); }"), "a process spawn must count as I/O");
    assert.ok(
        !fileDoesIo("fn pure() {}\n#[cfg(test)]\nmod tests { use std::fs; }"),
        "I/O in a test module must not count",
    );
    assert.ok(!fileDoesIo("// std::fs mentioned in a comment\nfn pure() {}"), "a comment must not count as I/O");
    assert.deepEqual(ioDrift(["a.rs", "b.rs"], ["a.rs"]).added, ["b.rs"], "a new I/O file must be flagged");
    assert.deepEqual(ioDrift(["a.rs"], ["a.rs", "c.rs"]).stale, ["c.rs"], "a baseline entry that left I/O must be flagged");

    console.log(
        "ok: boundary-guard self-test passed (real tree clean; injected domain->model-local, subscriber and I/O-drift cases are rejected)",
    );
}

function main() {
    if (process.argv.includes("--self-test")) {
        selfTest();
        return;
    }
    if (process.argv.includes("--print-io-files")) {
        console.log(JSON.stringify(domainIoFiles(), null, 2));
        return;
    }

    let failed = false;

    const offenders = findOffenders(domainNormalDeps());
    if (offenders.length > 0) {
        console.error(`error: cronus-domain has forbidden normal dependencies: ${offenders.join(", ")}`);
        console.error(`allowlist: ${[...ALLOWED].join(", ")}`);
        failed = true;
    } else {
        console.log(`ok: cronus-domain's normal dependencies are within the allowlist (${[...ALLOWED].join(", ")})`);
    }

    const drift = ioDrift(domainIoFiles(), readIoBaseline());
    if (drift.added.length > 0) {
        console.error(`error: cronus-domain files gained filesystem/process/network I/O outside the baseline: ${drift.added.join(", ")}`);
        console.error("move the I/O behind a contract seam into an adapter crate; do not add the file to scripts/domain-io-baseline.json");
        failed = true;
    }
    if (drift.stale.length > 0) {
        console.error(`error: baseline entries no longer do I/O — remove them from scripts/domain-io-baseline.json (the baseline only shrinks): ${drift.stale.join(", ")}`);
        failed = true;
    }
    if (drift.added.length === 0 && drift.stale.length === 0) {
        console.log(`ok: cronus-domain's I/O files match the shrink-only baseline (${readIoBaseline().length} files)`);
    }

    if (failed) {
        process.exit(1);
    }
}

main();
