#!/usr/bin/env bun
// Smoke test of a musl Bun running on the aphrody-libc overlay: --version,
// -e, offline `bun install` of a file: dependency, Bun.serve + fetch, and
// string/sort-heavy JS. Each step runs under LD_PRELOAD (dynamic musl Bun);
// the first step checks /proc/self/maps so a silently ignored preload fails.
//
//   bun aphrody/conformance/bun-smoke.ts --bun <path to musl bun> \
//     --preload /usr/lib/libaphrody_libc.so.0
//   bun aphrody/conformance/bun-smoke.ts --bun <bun linked with libaphrody_libc.a> --static
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values: opts } = parseArgs({
  options: {
    bun: { type: "string" },
    preload: { type: "string" },
    static: { type: "boolean", default: false },
  },
});
if (!opts.bun || (!opts.preload && !opts.static)) {
  console.error("bun-smoke.ts: --bun <path> and --preload <libaphrody_libc.so> (or --static)");
  process.exit(2);
}
const bun = resolve(opts.bun);
const env: Record<string, string | undefined> = { ...process.env, BUN_DEBUG_QUIET_LOGS: "1", NO_COLOR: "1" };
if (opts.preload) env.LD_PRELOAD = resolve(opts.preload);

const dir = mkdtempSync(join(tmpdir(), "aphrody-libc-smoke-"));
let failed = 0;

async function step(name: string, cmd: string[], check: (out: string) => boolean, cwd = dir) {
  const p = Bun.spawn({ cmd, cwd, env, stdout: "pipe", stderr: "pipe" });
  const [out, err, code] = await Promise.all([p.stdout.text(), p.stderr.text(), p.exited]);
  const ok = code === 0 && check(out);
  if (!ok) {
    failed++;
    console.error(`FAIL ${name} (exit ${code})\n--- stdout\n${out}\n--- stderr\n${err}`);
  } else console.log(`ok   ${name}`);
}

if (opts.preload) {
  await step(
    "overlay is mapped",
    [bun, "-e", `console.log(require("fs").readFileSync("/proc/self/maps","utf8").includes("libaphrody_libc"))`],
    out => out.trim() === "true",
  );
}

await step("--version", [bun, "--version"], out => /^\d+\.\d+\.\d+/.test(out.trim()));

await step(
  "-e strings, sort, JSON, regex",
  [
    bun,
    "-e",
    `
    const words = Array.from({ length: 20000 }, (_, i) => "w" + ((i * 7919) % 20000).toString(36).padStart(5, "0"));
    const sorted = [...words].sort();
    for (let i = 1; i < sorted.length; i++) if (sorted[i - 1] > sorted[i]) throw new Error("sort");
    const big = words.join(",");
    if (big.indexOf("w00000") < 0 || big.lastIndexOf(",") !== big.length - 7) throw new Error("indexOf");
    const json = JSON.parse(JSON.stringify({ words }));
    if (json.words.length !== 20000) throw new Error("json");
    if (!/w0{4}1/.test(big)) throw new Error("regex");
    const buf = Buffer.from(big);
    if (buf.indexOf("w00001") < 0 || Buffer.compare(buf, Buffer.from(big)) !== 0) throw new Error("buffer");
    console.log("ok");
    `,
  ],
  out => out.trim() === "ok",
);

await Bun.write(join(dir, "dep", "package.json"), JSON.stringify({ name: "aphrody-libc-dep", version: "1.0.0" }));
await Bun.write(join(dir, "dep", "index.js"), `module.exports = "dep-ok";`);
await Bun.write(
  join(dir, "app", "package.json"),
  JSON.stringify({ name: "app", version: "1.0.0", dependencies: { "aphrody-libc-dep": "file:../dep" } }),
);
// Unreachable registry: only the file: dependency can resolve.
await step(
  "install offline (file: dependency)",
  [bun, "install", "--no-save", "--cache-dir", join(dir, "cache"), "--registry", "http://127.0.0.1:9/"],
  () => true,
  join(dir, "app"),
);
await step(
  "require installed dependency",
  [bun, "-e", `console.log(require("aphrody-libc-dep"))`],
  out => out.trim() === "dep-ok",
  join(dir, "app"),
);

await step(
  "Bun.serve + fetch",
  [
    bun,
    "-e",
    `
    using server = Bun.serve({ port: 0, fetch: req => new Response("hello " + new URL(req.url).pathname) });
    const res = await fetch(server.url + "libc");
    console.log(await res.text());
    `,
  ],
  out => out.trim() === "hello /libc",
);

rmSync(dir, { recursive: true, force: true });
console.log(failed === 0 ? "bun smoke: all steps passed" : `bun smoke: ${failed} step(s) failed`);
process.exit(failed === 0 ? 0 : 1);
