import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFile, writeFile, mkdir, readdir } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const repository = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
function ensure(condition, category) {
  if (!condition) throw new Error(category);
}

const target = resolve(repository, process.env.CARGO_TARGET_DIR || "target");
const stamp = resolve(target, "browser-build.json");
export async function identity() {
  const git = (args) =>
    execFileSync("git", args, { cwd: repository, maxBuffer: 32 * 1024 * 1024 });
  const revision = git(["rev-parse", "HEAD"]).toString().trim();
  const hash = createHash("sha256")
    .update(revision)
    .update(git(["diff", "HEAD", "--binary"]));
  for (const path of git(["ls-files", "--others", "--exclude-standard", "-z"])
    .toString()
    .split("\0")
    .filter(Boolean)
    .sort()) {
    hash.update(path).update(await readFile(resolve(repository, path)));
  }
  return { revision, sourceDigest: hash.digest("hex") };
}
async function wasmDigest() {
  const dist = resolve(repository, "apps/labello-wasm/dist");
  const files = (await readdir(dist)).filter((name) => name.endsWith(".wasm"));
  ensure(files.length === 1, "build.wasm_asset");
  return createHash("sha256")
    .update(await readFile(resolve(dist, files[0])))
    .digest("hex");
}
export async function verifiedBuild() {
  const built = JSON.parse(await readFile(stamp));
  const source = await identity();
  ensure(
    built.revision === source.revision &&
      built.sourceDigest === source.sourceDigest &&
      built.wasmSha256 === (await wasmDigest()),
    "build.stale",
  );
  return built;
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    ensure(process.argv.length === 2, "build.arguments");
    await mkdir(target, { recursive: true });
    await writeFile(
      stamp,
      JSON.stringify(
        { ...(await identity()), wasmSha256: await wasmDigest() },
        null,
        2,
      ),
    );
  } catch {
    process.stderr.write("browser.build: stamp_failed\n");
    process.exitCode = 1;
  }
}
