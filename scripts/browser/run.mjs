import { chromium } from "playwright";
import { verifiedBuild } from "./build.mjs";
import { writeFile, mkdir, readFile, rm } from "node:fs/promises";
import { resolve } from "node:path";
import { spawn, execFileSync } from "node:child_process";
import { fixture, cleanup, repository, ensure, GateError } from "./support.mjs";
import {
  annotation,
  review,
  migration,
  admin,
  skeletons,
} from "./workflows.mjs";
import { display, zoom, extensionPath } from "./display.mjs";
import { recovery } from "./failures.mjs";

async function main() {
  const update = process.argv.includes("--update");
  const injection = process.argv
    .find((argument) => argument.startsWith("--inject="))
    ?.split("=")[1];
  const selected = process.argv
    .find((argument) => argument.startsWith("--workflow="))
    ?.split("=")[1];
  ensure(
    process.argv
      .slice(2)
      .every(
        (argument) =>
          argument === "--update" ||
          /^--inject=(startup|network|visual)$/.test(argument) ||
          /^--workflow=(annotation|review|migration|admin|display|zoom|recovery|skeletons)$/.test(
            argument,
          ),
      ),
    "arguments.invalid",
  );
  ensure(!(injection && update), "arguments.injection_update");
  if (!selected && !injection)
    await rm(resolve(repository, "scripts/browser/artifacts"), {
      recursive: true,
      force: true,
    });
  const build = await verifiedBuild().catch(() => {
    throw new GateError("build.unavailable_or_stale");
  });
  const results = [];
  const cleanupResults = [];
  const revision = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: repository,
    encoding: "utf8",
  }).trim();
  const dirty = !!execFileSync("git", ["status", "--porcelain"], {
    cwd: repository,
    encoding: "utf8",
  }).trim();
  let failure;
  for (const [scenario, workflow, dpr = 1] of [
    ["boxes", annotation],
    ["review", review],
    ["migration", migration],
    ["boxes", admin],
    ["boxes", display, 1],
    ["boxes", display, 2],
    ["boxes", display, 3],
    ["boxes", zoom],
    ["boxes", recovery],
    ["skeletons", skeletons],
  ]) {
    if (
      (selected && selected !== workflow.name) ||
      (injection && workflow !== annotation)
    )
      continue;
    let environment, browser, context;
    try {
      environment = await fixture(scenario);
      let page;
      if (workflow === zoom) {
        context = await chromium.launchPersistentContext(
          resolve(environment.directory, "profile"),
          {
            channel: "chromium",
            headless: true,
            viewport: { width: 1440, height: 1000 },
            args: [
              `--disable-extensions-except=${extensionPath}`,
              `--load-extension=${extensionPath}`,
            ],
          },
        );
        page = context.pages()[0];
      } else {
        // Chromium's native device-pixel box and its emulated DPR must agree.
        browser = await chromium.launch({
          headless: true,
          args: [`--force-device-scale-factor=${dpr}`],
        });
        page = await browser.newPage({
          viewport: {
            width: dpr === 3 ? 1288 : 1440,
            height: workflow === display ? 820 : 1000,
          },
          deviceScaleFactor: dpr,
        });
      }
      page.setDefaultTimeout(15_000);
      if (injection === "startup")
        await page.route("**/*.wasm", (route) => route.abort("failed"));
      if (injection === "network")
        await page.route("**/auth/local-admin", (route) =>
          route.abort("failed"),
        );
      let timer;
      try {
        results.push(
          await Promise.race([
            workflow(page, environment, {
              update,
              dpr,
              injectMismatch: injection === "visual",
            }),
            new Promise((_, reject) => {
              timer = setTimeout(
                () => reject(new GateError("workflow.timeout")),
                120_000,
              );
            }),
          ]),
        );
      } finally {
        clearTimeout(timer);
      }
      process.stdout.write(`browser.${workflow.name}: passed\n`);
    } catch (error) {
      failure = error.category || "browser.driver_failure";
      process.stderr.write(`browser.${workflow.name}: ${failure}\n`);
    } finally {
      for (const close of [
        () => context?.close(),
        () => browser?.close(),
        async () => {
          if (environment) cleanupResults.push(await cleanup(environment));
        },
      ]) {
        try {
          await close();
        } catch {
          failure = "cleanup.incomplete";
        }
      }
    }
    if (failure) break;
  }
  try {
    if (!failure && !selected && !injection && !update) {
      for (const kind of ["startup", "network", "visual"]) {
        const code = await new Promise((resolve, reject) => {
          const child = spawn(
            process.execPath,
            ["scripts/browser/run.mjs", `--inject=${kind}`],
            { cwd: repository, stdio: "ignore" },
          );
          child.once("error", reject);
          child.once("exit", resolve);
        });
        const report = JSON.parse(
          await readFile(
            resolve(
              repository,
              "scripts/browser/artifacts",
              `report-injected-${kind}.json`,
            ),
          ),
        );
        const expected = {
          startup: ["wasm.startup_failure", "wasm.startup"],
          network: ["browser.unexpected_failure"],
          visual: ["visual.mismatch"],
        }[kind];
        ensure(
          code === 1 &&
            expected.includes(report.failure) &&
            report.cleanup.length === 1 &&
            Object.values(report.cleanup[0]).every((value) => value === true),
          "injection.failed_to_reject_and_cleanup",
        );
        results.push({
          workflow: `injection-${kind}`,
          rejected: true,
          cleanup: report.cleanup,
        });
        process.stdout.write(
          `browser.injection-${kind}: rejected and cleaned up\n`,
        );
      }
    }
  } catch (error) {
    failure = error.category || "injection.driver_failure";
  }
  await mkdir(resolve(repository, "scripts/browser/artifacts"), {
    recursive: true,
  });
  const reportName = injection
    ? `report-injected-${injection}.json`
    : selected
      ? `report-${selected}.json`
      : "report.json";
  await writeFile(
    resolve(repository, "scripts/browser/artifacts", reportName),
    JSON.stringify(
      {
        fixture: "labello-browser-v1",
        revision,
        dirty,
        build,
        results,
        failure,
        cleanup: cleanupResults,
      },
      null,
      2,
    ),
  );
  if (failure) process.exitCode = 1;
}
main().catch((error) => {
  process.stderr.write(
    `browser.gate: ${error instanceof GateError ? error.category : "browser.driver_failure"}\n`,
  );
  process.exitCode = 1;
});
