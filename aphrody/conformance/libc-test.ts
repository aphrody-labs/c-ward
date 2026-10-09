#!/usr/bin/env bun
// musl libc-test with and without the aphrody-libc overlay; fails when the
// overlay adds a failure that plain musl does not have (musl itself carries a
// few known failures, so the baseline is measured, not assumed empty).
//
//   bun aphrody/conformance/libc-test.ts \
//     --archive /usr/lib/libaphrody_libc.a      # static replacement (dynamic + -static tests)
//     [--preload /usr/lib/libaphrody_libc.so.0] # extra run under LD_PRELOAD
//     [--src <libc-test checkout>] [--jobs N] [--keep]
//
// Runs on Alpine (musl). Needs make, a C compiler and git (unless --src).
import { $ } from "bun";
import { cpSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { cpus, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values: opts } = parseArgs({
  options: {
    archive: { type: "string" },
    preload: { type: "string" },
    src: { type: "string" },
    jobs: { type: "string", default: String(cpus().length) },
    keep: { type: "boolean", default: false },
  },
});

if (!opts.archive && !opts.preload) {
  console.error("libc-test.ts: pass --archive <libaphrody_libc.a> and/or --preload <libaphrody_libc.so>");
  process.exit(2);
}
for (const p of [opts.archive, opts.preload]) {
  if (p && !existsSync(p)) {
    console.error(`libc-test.ts: ${p} does not exist`);
    process.exit(2);
  }
}

const work = mkdtempSync(join(tmpdir(), "aphrody-libc-test-"));
const pristine = join(work, "src");
if (opts.src) {
  cpSync(resolve(opts.src), pristine, { recursive: true });
} else {
  const mirrors = ["https://repo.or.cz/libc-test.git", "git://repo.or.cz/libc-test.git"];
  let cloned = false;
  for (const url of mirrors) {
    if ((await $`git clone --depth 1 ${url} ${pristine}`.nothrow()).exitCode === 0) {
      cloned = true;
      break;
    }
    rmSync(pristine, { recursive: true, force: true });
  }
  if (!cloned) {
    console.error("libc-test.ts: could not clone libc-test; pass --src");
    process.exit(2);
  }
}

type Run = { name: string; failures: Set<string>; report: string };

/** `FAIL src/functional/foo.exe [status 1]` → `src/functional/foo.exe`. */
function failures(report: string): Set<string> {
  const out = new Set<string>();
  for (const line of report.split("\n")) {
    const m = /^FAIL (\S+)/.exec(line);
    if (m) out.add(m[1]);
  }
  return out;
}

async function run(name: string, extraMak: string, env: Record<string, string> = {}): Promise<Run> {
  const dir = join(work, name);
  cpSync(pristine, dir, { recursive: true });
  const def = await Bun.file(join(dir, "config.mak.def")).text();
  await Bun.write(join(dir, "config.mak"), def + extraMak);
  const t0 = performance.now();
  const r = await $`make -k -j${opts.jobs} run`
    .cwd(dir)
    .env({ ...process.env, ...env })
    .quiet()
    .nothrow();
  const reportFile = Bun.file(join(dir, "src", "REPORT"));
  if (!(await reportFile.exists())) {
    console.error(r.stderr.toString().slice(-4000));
    throw new Error(`${name}: make produced no src/REPORT (exit ${r.exitCode})`);
  }
  const report = await reportFile.text();
  const f = failures(report);
  console.log(`${name}: ${f.size} failing tests (${((performance.now() - t0) / 1000).toFixed(0)} s)`);
  return { name, failures: f, report };
}

const base = await run("musl", "");
const runs: Run[] = [];
if (opts.archive) {
  runs.push(await run("overlay-static", `\nLDLIBS += ${resolve(opts.archive)}\n`));
}
if (opts.preload) {
  runs.push(await run("overlay-preload", "", { LD_PRELOAD: resolve(opts.preload) }));
}

let regressions = 0;
for (const r of runs) {
  const added = [...r.failures].filter(t => !base.failures.has(t)).sort();
  const fixed = [...base.failures].filter(t => !r.failures.has(t)).sort();
  if (fixed.length) console.log(`${r.name}: passes ${fixed.length} test(s) musl fails: ${fixed.join(" ")}`);
  if (added.length) {
    regressions += added.length;
    console.error(`${r.name}: ${added.length} new failure(s):`);
    for (const t of added) {
      for (const line of r.report.split("\n")) if (line.includes(t)) console.error(`  ${line}`);
    }
  }
}

if (opts.keep) console.log(`kept ${work}`);
else rmSync(work, { recursive: true, force: true });

console.log(regressions === 0 ? "libc-test: no regression against musl" : `libc-test: ${regressions} regression(s)`);
process.exit(regressions === 0 ? 0 : 1);
